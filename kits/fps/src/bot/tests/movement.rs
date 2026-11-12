//! Movement tests: jumping, strafing, obstacle handling, aim turns, and stuck detection.

use std::f32::consts::TAU;

use app::ViewAngles;

use super::*;
use crate::bot::aim::{angles_towards, wrap_angle};
use crate::bot::brain::next_strafe_gap;
use crate::command::look_direction;

#[test]
fn an_attacking_bot_on_the_ground_jumps_at_its_chance() -> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let nav = empty()?;
    let senses = FakeSenses::seeing();
    let jumps = |chance: f32| -> Result<u32, Box<dyn std::error::Error>> {
        let cfg = BotConfig::new(BotParams {
            jump_chance: chance,
            ..steady_params()
        })?;
        let mut brain = BotBrain::new();
        let mut rng = SimRng::new(11, 0);
        let mut count = 0;
        for _ in 0..600 {
            let input = fx.input(Some(fx.ahead(10.0)), ViewAngles::ZERO);
            count += u32::from(
                brain
                    .think(&cfg, &input, &nav, &senses, &mut rng)?
                    .jump_held(),
            );
        }
        Ok(count)
    };
    assert_eq!(jumps(0.0)?, 0, "a zero chance never jumps");
    assert_eq!(jumps(1.0)?, 600, "a certain chance jumps every tick");
    let sometimes = jumps(0.05)?;
    assert!(
        (5..=80).contains(&sometimes),
        "about 30 of 600, got {sometimes}"
    );
    Ok(())
}

#[test]
fn the_strafe_gap_varies_between_half_and_one_and_a_half_periods()
-> Result<(), Box<dyn std::error::Error>> {
    let cfg = BotConfig::new(params())?;
    let mut rng = SimRng::new(5, 0);
    let gaps: Vec<u32> = (0..200).map(|_| next_strafe_gap(&cfg, &mut rng)).collect();
    let period = cfg.strafe_period_ticks;
    assert!(
        gaps.iter()
            .all(|g| *g >= period / 2 && *g < period / 2 + period)
    );
    assert!(
        gaps.iter().any(|g| Some(g) != gaps.first()),
        "the gaps are not all equal"
    );
    Ok(())
}

#[test]
fn low_obstacle_makes_a_grounded_bot_jump_once() -> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(8, 0);
    let senses = FakeSenses {
        visible: false,
        blocked_low: true,
        blocked_high: false,
        path_clear: true,
    };
    let nav = graph(vec![Vec3::new(0.0, 0.0, -10.0)], &[])?;
    let first = brain.think(
        &cfg,
        &fx.input(None, ViewAngles::ZERO),
        &nav,
        &senses,
        &mut rng,
    )?;
    assert!(first.jump_pressed());
    let second = brain.think(
        &cfg,
        &fx.input(None, ViewAngles::ZERO),
        &nav,
        &senses,
        &mut rng,
    )?;
    assert!(!second.jump_pressed());
    assert!(second.jump_held());
    Ok(())
}

#[test]
fn fully_blocked_bot_turns_away_instead_of_advancing() -> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(9, 0);
    let senses = FakeSenses {
        visible: false,
        blocked_low: true,
        blocked_high: true,
        path_clear: true,
    };
    let nav = graph(vec![Vec3::new(0.0, 0.0, -10.0)], &[])?;
    let command = brain.think(
        &cfg,
        &fx.input(None, ViewAngles::ZERO),
        &nav,
        &senses,
        &mut rng,
    )?;
    assert_eq!(command.move_axis().y, 0.0);
    assert_ne!(command.move_axis().x, 0.0);
    Ok(())
}

#[test]
fn a_visible_enemy_is_tracked_at_turn_speed_not_at_aim_speed()
-> Result<(), Box<dyn std::error::Error>> {
    let nav = corridor()?;
    let fx = Fixture::new()?;
    // The aim error shrinks very slowly, but the view must still turn to the enemy quickly.
    let mut slow_aim = params();
    slow_aim.aim_speed_rad_per_s = 0.01;
    slow_aim.turn_speed = 3.0;
    let cfg = BotConfig::new(slow_aim)?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(13, 0);
    // An enemy 40 degrees to the east of the bot's forward direction, within its field of view.
    let enemy = Target {
        eye: Vec3::new(6.44, 1.7, -7.65),
        body: fx.enemy,
    };
    let command = brain.think(
        &cfg,
        &fx.input(Some(enemy), ViewAngles::ZERO),
        &nav,
        &FakeSenses::seeing(),
        &mut rng,
    )?;
    assert!(
        command.view().yaw() < -0.02,
        "the view turned only {} rad towards the enemy",
        command.view().yaw()
    );
    Ok(())
}

#[test]
fn a_stuck_bot_drops_its_enemy_and_repicks_a_patrol() -> Result<(), Box<dyn std::error::Error>> {
    let nav = corridor()?;
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(12, 0);
    brain.alert(&cfg, Vec3::new(20.0, 1.7, 0.0), &mut rng);
    let input = fx.input(None, ViewAngles::ZERO);
    for _ in 0..(cfg.stuck_ticks + 2) {
        brain.think(&cfg, &input, &nav, &FakeSenses::unseen(), &mut rng)?;
    }
    assert!(!brain.engaged);
    assert!(brain.last_seen.is_none());
    assert!(matches!(brain.phase(), Phase::Patrol { .. }));
    Ok(())
}

#[test]
fn a_bot_making_progress_is_not_stuck() -> Result<(), Box<dyn std::error::Error>> {
    let nav = corridor()?;
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(BotParams {
        stuck_ticks: 5,
        ..params()
    })?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(13, 0);
    brain.alert(&cfg, Vec3::new(20.0, 1.7, 0.0), &mut rng);
    let mut input = fx.input(None, ViewAngles::ZERO);
    for _ in 0..20 {
        brain.think(&cfg, &input, &nav, &FakeSenses::unseen(), &mut rng)?;
        // Half a metre a tick is well over the progress threshold in every stuck window.
        input.eye.x += 0.5;
    }
    assert!(brain.engaged);
    Ok(())
}

#[test]
fn angles_towards_matches_the_view_convention() -> Result<(), KitError> {
    let direction = Vec3::new(-1.0, 0.3, -0.4).normalize();
    let (yaw, pitch) = angles_towards(direction);
    let view = ViewAngles::new(yaw, pitch)?;
    assert!(look_direction(view).dot(direction) > 0.999);
    Ok(())
}

#[test]
fn wrap_angle_keeps_turns_short() {
    assert!((wrap_angle(TAU - 0.1) - (-0.1)).abs() < 1e-5);
    assert!((wrap_angle(-TAU + 0.1) - 0.1).abs() < 1e-5);
}
