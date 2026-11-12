//! The brain's state and its decision for each tick: the phase, the input the brain sees, and
//! [`BotBrain::think`], which turns perception and memory into one [`MoveCommand`].
//!
//! Invariants: the brain depends only on its input, its own state and its RNG stream, so equal
//! seeds and equal inputs give equal commands; an attacking bot fires only while it sees its target.

use app::ViewAngles;
use engine_core::SimRng;
use engine_physics::BodyId;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::config::BotConfig;
use super::senses::{Senses, Target, sees};
use crate::command::{MoveCommand, Trigger};
use crate::error::KitError;
use crate::waypoints::{WaypointId, Waypoints, horizontal};

/// How far a bot probes ahead for an obstacle, in metres. A tolerance for the probe, not a
/// gameplay number: the bot looks just beyond its own body.
const OBSTACLE_PROBE_M: f32 = 0.6;

/// A patrol route segment longer than this distance is worth sprinting, in metres.
const ROUTE_SPRINT_DISTANCE_M: f32 = 8.0;

/// Where the brain is in its state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Nothing to do: the world has no waypoints.
    Idle,
    /// Walking the route to this waypoint, in order around the list.
    Patrol {
        /// The waypoint being patrolled towards.
        waypoint: WaypointId,
    },
    /// Moving towards an enemy seen (or last seen) but not yet in engage range.
    Chase,
    /// Within engage range of a visible enemy: strafing and firing.
    Attack,
}

/// What the brain sees and knows this tick. Built by [`bot_system`] from the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BotInput {
    /// Capsule centre in world space.
    pub centre: Vec3,
    /// Eye position in world space.
    pub eye: Vec3,
    /// The bot's own body, ignored by its line-of-sight queries.
    pub body: BodyId,
    /// The view the bot faces at the start of the tick.
    pub view: ViewAngles,
    /// Whether the bot stands on walkable ground.
    pub grounded: bool,
    /// Whether the magazine is empty, which makes the bot reload when it has no enemy in sight.
    pub ammo_empty: bool,
    /// The nearest living enemy, if any.
    pub target: Option<Target>,
    /// Fixed timestep in seconds.
    pub dt: f32,
}

/// The brain's memory between ticks. Pure state; all decisions are made by [`BotBrain::think`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BotBrain {
    pub(super) phase: Phase,
    pub(super) engaged: bool,
    pub(super) sighted_for: u32,
    pub(super) lost_for: u32,
    pub(super) aim_error: f32,
    pub(super) aim_side: f32,
    pub(super) strafe_sign: f32,
    pub(super) strafe_timer: u32,
    pub(super) strafe_until: u32,
    pub(super) last_seen: Option<Vec3>,
    pub(super) jump_held: bool,
    pub(super) moving: bool,
    pub(super) anchor: Vec3,
    pub(super) stalled_for: u32,
}

impl Default for BotBrain {
    fn default() -> Self {
        Self::new()
    }
}

impl BotBrain {
    /// A brain that has not yet seen anything.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            engaged: false,
            sighted_for: 0,
            lost_for: 0,
            aim_error: 0.0,
            aim_side: 0.0,
            strafe_sign: 1.0,
            strafe_timer: 0,
            strafe_until: 0,
            last_seen: None,
            jump_held: false,
            moving: false,
            anchor: Vec3::ZERO,
            stalled_for: 0,
        }
    }

    /// The current state-machine phase.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether the bot has an enemy in mind: one it sees, or one it saw or heard recently.
    #[must_use]
    pub const fn is_engaged(&self) -> bool {
        self.engaged
    }

    /// Whether the bot currently sees an enemy, excluding a remembered or heard position.
    #[must_use]
    pub const fn is_sighted(&self) -> bool {
        self.sighted_for > 0
    }

    /// Last known enemy position, from sight or an alert.
    #[must_use]
    pub const fn last_known_enemy_position(&self) -> Option<Vec3> {
        self.last_seen
    }

    /// The aim error still in effect, in radians.
    #[must_use]
    pub const fn aim_error(&self) -> f32 {
        self.aim_error
    }

    /// Decides one tick: updates perception, progress and the phase, then returns the command for
    /// the tick.
    ///
    /// `nav` is the patrol and route graph. `rng` is the bot's own stream; it is drawn from when
    /// the bot first engages (to pick the side its aim error leans to and its strafe start) and
    /// when it is stuck (to pick a new patrol waypoint).
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] when the senses fail or a command value is rejected.
    pub fn think<S: Senses>(
        &mut self,
        cfg: &BotConfig,
        input: &BotInput,
        nav: &Waypoints,
        senses: &S,
        rng: &mut SimRng,
    ) -> Result<MoveCommand, KitError> {
        let offset = input.target.map(|target| target.eye - input.eye);
        let distance = offset.map(Vec3::length);
        let visible = match (input.target, offset) {
            (Some(target), Some(offset)) => sees(cfg, input, target, offset, senses)?,
            _ => false,
        };
        self.update_sight(cfg, input, visible, rng);
        self.phase = self.next_phase(cfg, visible, distance, nav);
        self.watch_progress(cfg, input, nav, rng)?;

        let goal = self.goal(cfg, input, visible, nav, senses)?;
        let (heading, remaining) = match goal {
            Some(goal) => (
                horizontal(goal.step - input.eye),
                horizontal(goal.destination - input.eye).length(),
            ),
            None => (Vec3::ZERO, 0.0),
        };
        let mut move_axis = self.plan_move(cfg, distance, remaining, rng);
        // Wanting to advance counts as moving even when an obstacle turns the bot aside below,
        // so a bot pinned against a wall is seen as stuck.
        self.moving =
            move_axis.y > 0.0 && matches!(self.phase, Phase::Patrol { .. } | Phase::Chase);
        // An attacking bot hops now and then, as a player does to throw off aim.
        let mut jump_wanted = matches!(self.phase, Phase::Attack)
            && input.grounded
            && rng.next_f32() < cfg.jump_chance;
        if move_axis.y > 0.0 {
            let direction = heading.normalize_or_zero();
            if direction != Vec3::ZERO {
                let low = senses.blocked(input.body, input.centre, direction, OBSTACLE_PROBE_M)?;
                let high = senses.blocked(input.body, input.eye, direction, OBSTACLE_PROBE_M)?;
                if low && high {
                    self.strafe_sign = -self.strafe_sign;
                    move_axis = Vec2::new(self.strafe_sign, 0.0);
                } else if low {
                    jump_wanted = input.grounded;
                }
            }
        }
        let jump_pressed = jump_wanted && !self.jump_held;
        self.jump_held = jump_wanted;

        let (view, aimed) = self.turn(cfg, input, visible, heading)?;
        let fire = matches!(self.phase, Phase::Attack)
            && visible
            && self.sighted_for >= cfg.reaction_ticks
            && aimed;
        let reload = input.ammo_empty && !visible;
        let sprint = matches!(self.phase, Phase::Patrol { .. })
            && input.target.is_none()
            && remaining > ROUTE_SPRINT_DISTANCE_M
            && move_axis.y > 0.5;
        MoveCommand::new(
            move_axis,
            view,
            jump_pressed,
            jump_wanted,
            if fire {
                Trigger::Held
            } else {
                Trigger::Released
            },
            reload,
        )
        .map(|command| command.with_movement(sprint, false))
    }

    /// The strafe and forward axes for this tick. Attacking strafes and flips direction every
    /// strafe period. Patrolling and chasing move forward until the destination is within arrive
    /// radius; a route step on the way is never a reason to stop.
    fn plan_move(
        &mut self,
        cfg: &BotConfig,
        distance: Option<f32>,
        remaining: f32,
        rng: &mut SimRng,
    ) -> Vec2 {
        match self.phase {
            Phase::Idle => Vec2::ZERO,
            Phase::Attack => {
                self.strafe_timer = self.strafe_timer.saturating_add(1);
                if self.strafe_timer >= self.strafe_until {
                    self.strafe_timer = 0;
                    self.strafe_until = next_strafe_gap(cfg, rng);
                    self.strafe_sign = -self.strafe_sign;
                }
                let close = distance.is_some_and(|d| d <= cfg.stop_distance);
                Vec2::new(self.strafe_sign, if close { 0.0 } else { 1.0 })
            }
            Phase::Patrol { .. } | Phase::Chase => {
                let forward = if remaining <= cfg.arrive_radius {
                    0.0
                } else {
                    1.0
                };
                Vec2::new(0.0, forward)
            }
        }
    }
}

/// Ticks until the next strafe change: half to one and a half strafe periods, drawn from `rng`.
pub(super) fn next_strafe_gap(cfg: &BotConfig, rng: &mut SimRng) -> u32 {
    cfg.strafe_period_ticks / 2 + rng.next_u32() % cfg.strafe_period_ticks.max(1)
}
