//! Unit tests for the bot: shared fixtures (a scripted set of senses, a physics fixture and sample
//! waypoint worlds), configuration validation, and the bot component. The behaviour tests live in
//! `tests/`.

use app::ViewAngles;
use engine_core::SimRng;
use engine_physics::{BodyBuilder, BodyId, PhysicsConfig, PhysicsError, PhysicsWorld};
use glam::Vec3;
use proptest::prelude::*;

use super::*;
use crate::error::KitError;
use crate::waypoints::test_links::{LinkTable, open_rules};
use crate::waypoints::{WaypointId, Waypoints};

mod brain;
mod movement;
mod navigation;

/// Scripted perception: the answers are fixed.
struct FakeSenses {
    visible: bool,
    blocked_low: bool,
    blocked_high: bool,
    /// Whether the straight segments the route asks about are open. The fake world has no
    /// walls, so this is true unless a test says otherwise.
    path_clear: bool,
}

impl FakeSenses {
    const fn seeing() -> Self {
        Self {
            visible: true,
            blocked_low: false,
            blocked_high: false,
            path_clear: true,
        }
    }

    const fn unseen() -> Self {
        Self {
            visible: false,
            blocked_low: false,
            blocked_high: false,
            path_clear: true,
        }
    }
}

impl Senses for FakeSenses {
    fn can_see(
        &self,
        _observer: BodyId,
        _eye: Vec3,
        _target: BodyId,
        _target_eye: Vec3,
    ) -> Result<bool, KitError> {
        Ok(self.visible)
    }

    fn blocked(
        &self,
        _observer: BodyId,
        origin: Vec3,
        _direction: Vec3,
        _range: f32,
    ) -> Result<bool, KitError> {
        Ok(if origin.y > 1.0 {
            self.blocked_high
        } else {
            self.blocked_low
        })
    }

    fn clear(&self, _observer: Option<BodyId>, _from: Vec3, _to: Vec3) -> Result<bool, KitError> {
        Ok(self.path_clear)
    }
}

/// Two real body ids from a physics world, so the tests hold identifiers the physics crate
/// issued. The brain only compares them; the fake senses ignore them.
struct Fixture {
    me: BodyId,
    enemy: BodyId,
}

impl Fixture {
    fn new() -> Result<Self, PhysicsError> {
        let mut physics = PhysicsWorld::new(PhysicsConfig::new(1.0 / 60.0, Vec3::ZERO)?);
        let me = physics.insert_body(BodyBuilder::kinematic());
        let enemy = physics.insert_body(BodyBuilder::kinematic());
        Ok(Self { me, enemy })
    }

    /// The bot's input at standing height, grounded, with the given target and view.
    fn input(&self, target: Option<Target>, view: ViewAngles) -> BotInput {
        BotInput {
            centre: Vec3::new(0.0, 1.0, 0.0),
            eye: Vec3::new(0.0, 1.7, 0.0),
            body: self.me,
            view,
            grounded: true,
            ammo_empty: false,
            target,
            dt: 1.0 / 60.0,
        }
    }

    /// An enemy straight ahead (down -Z) at `distance` metres.
    fn ahead(&self, distance: f32) -> Target {
        Target {
            eye: Vec3::new(0.0, 1.7, -distance),
            body: self.enemy,
        }
    }

    /// An enemy straight behind (+Z) at `distance` metres.
    fn behind(&self, distance: f32) -> Target {
        Target {
            eye: Vec3::new(0.0, 1.7, distance),
            body: self.enemy,
        }
    }
}

fn params() -> BotParams {
    BotParams {
        sight_range: 30.0,
        fov_rad: 2.0,
        reaction_ticks: 5,
        aim_speed_rad_per_s: 4.0,
        aim_error_rad: 0.2,
        engage_range: 20.0,
        stop_distance: 3.0,
        strafe_period_ticks: 10,
        jump_chance: 0.0,
        turn_speed: 3.0,
        forget_ticks: 30,
        arrive_radius: 1.5,
        stuck_ticks: 60,
        stuck_progress: 0.3,
    }
}

/// The default parameters with no aim error, so a bot facing its target is on it at once.
fn steady_params() -> BotParams {
    BotParams {
        aim_error_rad: 0.0,
        ..params()
    }
}

/// Waypoints at `points` with the given links (pairs of list indices), built from scripted
/// links. Waypoints are placed on the x axis at height zero.
fn graph(points: Vec<Vec3>, links: &[(usize, usize)]) -> Result<Waypoints, KitError> {
    let count = points.len();
    let mut linked = vec![vec![false; count]; count];
    for &(a, b) in links {
        if let Some(cell) = linked.get_mut(a).and_then(|row| row.get_mut(b)) {
            *cell = true;
        }
        if let Some(cell) = linked.get_mut(b).and_then(|row| row.get_mut(a)) {
            *cell = true;
        }
    }
    let table = LinkTable::new(points.clone(), linked);
    Waypoints::build(points, &table, &open_rules()?)
}

/// A world with no waypoints.
fn empty() -> Result<Waypoints, KitError> {
    graph(Vec::new(), &[])
}

/// Three waypoints in a line, ten metres apart, linked only neighbour to neighbour.
fn corridor() -> Result<Waypoints, KitError> {
    graph(
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(20.0, 0.0, 0.0),
        ],
        &[(0, 1), (1, 2)],
    )
}

#[test]
fn config_rejects_engage_beyond_sight() {
    let mut bad = params();
    bad.engage_range = 40.0;
    assert!(BotConfig::new(bad).is_err());
}

#[test]
fn config_rejects_zero_strafe_period_and_fov() {
    let mut bad = params();
    bad.strafe_period_ticks = 0;
    assert!(BotConfig::new(bad).is_err());
    let mut no_fov = params();
    no_fov.fov_rad = 0.0;
    assert!(BotConfig::new(no_fov).is_err());
}

#[test]
fn config_rejects_zero_arrive_radius_and_stuck_ticks() {
    let mut no_arrive = params();
    no_arrive.arrive_radius = 0.0;
    assert!(BotConfig::new(no_arrive).is_err());
    let mut no_window = params();
    no_window.stuck_ticks = 0;
    assert!(BotConfig::new(no_window).is_err());
}

#[test]
fn config_rejects_a_jump_chance_outside_zero_to_one() {
    for bad in [-0.1, 1.1, f32::NAN] {
        let mut params = params();
        params.jump_chance = bad;
        assert!(BotConfig::new(params).is_err(), "{bad}");
    }
}

#[test]
fn idle_becomes_patrol_when_waypoints_exist() -> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(1, 0);
    let nav = graph(vec![Vec3::new(0.0, 0.0, 10.0)], &[])?;
    brain.think(
        &cfg,
        &fx.input(None, ViewAngles::ZERO),
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
fn idle_stays_idle_without_waypoints() -> Result<(), Box<dyn std::error::Error>> {
    let fx = Fixture::new()?;
    let cfg = BotConfig::new(params())?;
    let mut brain = BotBrain::new();
    let mut rng = SimRng::new(1, 0);
    brain.think(
        &cfg,
        &fx.input(None, ViewAngles::ZERO),
        &empty()?,
        &FakeSenses::unseen(),
        &mut rng,
    )?;
    assert_eq!(brain.phase(), Phase::Idle);
    Ok(())
}

#[test]
fn alerting_rejects_a_non_finite_position() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = BotConfig::new(params())?;
    let mut bot = Bot::new(cfg, SimRng::new(14, 0));
    assert!(bot.alert(Vec3::new(f32::NAN, 0.0, 0.0)).is_err());
    Ok(())
}

#[test]
fn retuning_keeps_the_brain_route_and_memory() -> Result<(), KitError> {
    let mut bot = Bot::new(BotConfig::new(params())?, SimRng::new(15, 0));
    bot.brain.engaged = true;
    bot.brain.sighted_for = 9;
    bot.brain.last_seen = Some(Vec3::new(4.0, 1.0, 2.0));
    let brain = bot.brain;
    let config = BotConfig::new(BotParams {
        sight_range: 40.0,
        engage_range: 30.0,
        ..params()
    })?;
    bot.set_config(config);
    assert_eq!(bot.config(), &config);
    assert_eq!(bot.brain, brain);
    Ok(())
}

proptest! {
    #[test]
    fn retuning_generated_valid_bot_configs_keeps_brain_state(
        sight_range in 2.0f32..100.0,
        engage_fraction in 0.1f32..=1.0,
    ) {
        let config = BotConfig::new(BotParams {
            sight_range,
            engage_range: sight_range * engage_fraction,
            ..params()
        }).map_err(|error| TestCaseError::fail(error.to_string()))?;
        let mut bot = Bot::new(
            BotConfig::new(params()).map_err(|error| TestCaseError::fail(error.to_string()))?,
            SimRng::new(16, 0),
        );
        bot.brain.sighted_for = 11;
        bot.brain.last_seen = Some(Vec3::new(1.0, 2.0, 3.0));
        let brain = bot.brain;
        bot.set_config(config);
        prop_assert_eq!(bot.config(), &config);
        prop_assert_eq!(bot.brain, brain);
    }
}
