//! Navigation tests: patrol routes through the waypoint graph, arrival at waypoints, and chasing a
//! point the bot was alerted to.

use super::*;
use crate::bot::navigation::route;

#[test]
fn patrol_routes_through_the_next_hop() -> Result<(), Box<dyn std::error::Error>> {
    // From waypoint 0 towards waypoint 2 the route passes waypoint 1, not the straight line.
    let nav = corridor()?;
    let fx = Fixture::new()?;
    let input = fx.input(None, ViewAngles::ZERO);
    let destination = Vec3::new(20.0, 0.0, 0.0);
    let step = route(&input, &nav, &FakeSenses::seeing(), destination)?;
    assert_eq!(step, Some(Vec3::new(10.0, 0.0, 0.0)));
    Ok(())
}

#[test]
fn route_heads_straight_for_a_destination_in_the_same_cell()
-> Result<(), Box<dyn std::error::Error>> {
    let nav = corridor()?;
    let fx = Fixture::new()?;
    let input = fx.input(None, ViewAngles::ZERO);
    let destination = Vec3::new(1.0, 0.0, 0.5);
    let step = route(&input, &nav, &FakeSenses::seeing(), destination)?;
    assert_eq!(step, Some(destination));
    Ok(())
}

#[test]
fn arriving_at_a_waypoint_advances_the_patrol() -> Result<(), Box<dyn std::error::Error>> {
    let nav = corridor()?;
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(10, 0);
    // The bot stands on waypoint 0, so the first tick takes the patrol on to waypoint 1.
    let input = fx.input(None, ViewAngles::ZERO);
    brain.think(&cfg, &input, &nav, &FakeSenses::unseen(), &mut rng)?;
    let next = nav.id(1).ok_or("waypoint 1")?;
    assert_eq!(brain.phase(), Phase::Patrol { waypoint: next });
    Ok(())
}

#[test]
fn an_alerted_bot_without_sight_chases_the_point_it_was_told()
-> Result<(), Box<dyn std::error::Error>> {
    let nav = corridor()?;
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(11, 0);
    brain.alert(&cfg, Vec3::new(20.0, 1.7, 0.0), &mut rng);
    assert_eq!(
        brain.last_known_enemy_position(),
        Some(Vec3::new(20.0, 1.7, 0.0))
    );
    let command = brain.think(
        &cfg,
        &fx.input(None, ViewAngles::ZERO),
        &nav,
        &FakeSenses::unseen(),
        &mut rng,
    )?;
    assert_eq!(brain.phase(), Phase::Chase);
    assert!(command.move_axis().y > 0.0);
    // The first hop is east (+X), which is a negative yaw in the view convention.
    assert!(command.view().yaw() < 0.0);
    Ok(())
}

#[test]
fn a_route_step_near_the_bot_does_not_stop_it_short_of_its_destination()
-> Result<(), Box<dyn std::error::Error>> {
    let nav = corridor()?;
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(12, 0);
    brain.alert(&cfg, Vec3::new(20.0, 1.7, 0.0), &mut rng);
    // Standing 0.5 m short of the waypoint at x = 10, which is the route's next step. That
    // step is within arrive radius, but the destination at x = 20 is not, so the bot walks on.
    // The next hop is not in sight from here, so the route steps back onto its own waypoint.
    let mut input = fx.input(None, ViewAngles::ZERO);
    input.centre = Vec3::new(9.5, 1.0, 0.0);
    input.eye = Vec3::new(9.5, 1.7, 0.0);
    let senses = FakeSenses {
        visible: false,
        blocked_low: false,
        blocked_high: false,
        path_clear: false,
    };
    let command = brain.think(&cfg, &input, &nav, &senses, &mut rng)?;
    assert!(
        command.move_axis().y > 0.0,
        "the bot stopped on a route step"
    );
    Ok(())
}
