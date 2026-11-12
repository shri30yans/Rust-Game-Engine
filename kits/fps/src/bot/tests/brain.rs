//! Brain tests: what a bot sees, when it engages, when it fires, and when it forgets an enemy.

use super::*;

#[test]
fn sighting_chases_then_attacks_inside_engage_range() -> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(2, 0);
    let senses = FakeSenses::seeing();
    let nav = empty()?;
    brain.think(
        &cfg,
        &fx.input(Some(fx.ahead(25.0)), ViewAngles::ZERO),
        &nav,
        &senses,
        &mut rng,
    )?;
    assert_eq!(brain.phase(), Phase::Chase);
    brain.think(
        &cfg,
        &fx.input(Some(fx.ahead(10.0)), ViewAngles::ZERO),
        &nav,
        &senses,
        &mut rng,
    )?;
    assert_eq!(brain.phase(), Phase::Attack);
    Ok(())
}

#[test]
fn reaction_delay_holds_fire_until_reaction_ticks_have_passed()
-> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(steady_params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(3, 0);
    let senses = FakeSenses::seeing();
    let nav = empty()?;
    let mut fired_on = Vec::new();
    for tick in 1..=10u32 {
        let command = brain.think(
            &cfg,
            &fx.input(Some(fx.ahead(10.0)), ViewAngles::ZERO),
            &nav,
            &senses,
            &mut rng,
        )?;
        if command.fire() {
            fired_on.push(tick);
        }
    }
    assert_eq!(fired_on.first().copied(), Some(cfg.reaction_ticks));
    Ok(())
}

#[test]
fn aim_error_shrinks_while_the_enemy_is_visible() -> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(4, 0);
    let senses = FakeSenses::seeing();
    let nav = empty()?;
    let mut previous = f32::INFINITY;
    for _ in 0..5 {
        brain.think(
            &cfg,
            &fx.input(Some(fx.ahead(10.0)), ViewAngles::ZERO),
            &nav,
            &senses,
            &mut rng,
        )?;
        assert!(brain.aim_error() <= previous);
        previous = brain.aim_error();
    }
    assert!(brain.aim_error() < cfg.aim_error_rad);
    Ok(())
}

#[test]
fn forgets_the_enemy_after_forget_ticks_without_sight() -> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(5, 0);
    let nav = graph(vec![Vec3::new(0.0, 0.0, 10.0)], &[])?;
    brain.think(
        &cfg,
        &fx.input(Some(fx.ahead(10.0)), ViewAngles::ZERO),
        &nav,
        &FakeSenses::seeing(),
        &mut rng,
    )?;
    for _ in 0..cfg.forget_ticks {
        brain.think(
            &cfg,
            &fx.input(Some(fx.ahead(10.0)), ViewAngles::ZERO),
            &nav,
            &FakeSenses::unseen(),
            &mut rng,
        )?;
    }
    assert_eq!(brain.phase(), Phase::Chase);
    brain.think(
        &cfg,
        &fx.input(Some(fx.ahead(10.0)), ViewAngles::ZERO),
        &nav,
        &FakeSenses::unseen(),
        &mut rng,
    )?;
    assert_eq!(
        brain.phase(),
        Phase::Patrol {
            waypoint: WaypointId::FIRST
        }
    );
    Ok(())
}

#[test]
fn never_fires_without_line_of_sight() -> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(steady_params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(6, 0);
    let blocked = FakeSenses {
        visible: false,
        blocked_low: true,
        blocked_high: true,
        path_clear: true,
    };
    let nav = empty()?;
    for _ in 0..100 {
        let command = brain.think(
            &cfg,
            &fx.input(Some(fx.ahead(10.0)), ViewAngles::ZERO),
            &nav,
            &blocked,
            &mut rng,
        )?;
        assert!(!command.fire());
    }
    Ok(())
}

#[test]
fn enemy_outside_the_field_of_view_is_not_seen() -> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(7, 0);
    brain.think(
        &cfg,
        &fx.input(Some(fx.behind(10.0)), ViewAngles::ZERO),
        &empty()?,
        &FakeSenses::seeing(),
        &mut rng,
    )?;
    assert_eq!(brain.phase(), Phase::Idle);
    Ok(())
}
