//! The brain's memory of enemies: when an engagement starts and ends, how long an enemy has been
//! in sight or out of it, and the phase that follows.
//!
//! Invariants: the first sighting starts an engagement; an engagement ends only after more than
//! `forget_ticks` ticks without sight; a disengaged bot patrols when the world has waypoints and is
//! idle when it has none.

use std::f32::consts::TAU;

use engine_core::SimRng;
use glam::Vec3;

use super::brain::{BotBrain, BotInput, Phase, next_strafe_gap};
use super::config::BotConfig;
use crate::waypoints::{WaypointId, Waypoints};

impl BotBrain {
    /// Tells the brain where an enemy is last known to be, without sight. The brain engages and
    /// chases that point through the graph. The game calls this when it knows of an enemy the bot
    /// cannot see, for example after a shot the bot heard.
    pub fn alert(&mut self, cfg: &BotConfig, position: Vec3, rng: &mut SimRng) {
        self.engage(cfg, rng);
        self.last_seen = Some(position);
        self.lost_for = 0;
    }

    /// Starts an engagement: resets the sight count and aim, and draws the aim side and the strafe
    /// start from `rng`.
    fn engage(&mut self, cfg: &BotConfig, rng: &mut SimRng) {
        self.engaged = true;
        self.sighted_for = 0;
        self.aim_error = cfg.aim_error_rad;
        self.aim_side = rng.next_f32() * TAU;
        self.strafe_sign = if rng.next_f32() < 0.5 { -1.0 } else { 1.0 };
        self.strafe_timer = 0;
        self.strafe_until = next_strafe_gap(cfg, rng);
    }

    /// Updates the sight memory: first sighting starts an engagement, seeing the enemy counts
    /// reaction time and shrinks the aim error, losing it counts towards forgetting.
    pub(super) fn update_sight(
        &mut self,
        cfg: &BotConfig,
        input: &BotInput,
        visible: bool,
        rng: &mut SimRng,
    ) {
        if visible {
            if !self.engaged {
                self.engage(cfg, rng);
            }
            self.sighted_for = self.sighted_for.saturating_add(1);
            self.lost_for = 0;
            self.aim_error = (self.aim_error - cfg.aim_speed_rad_per_s * input.dt).max(0.0);
            if let Some(target) = input.target {
                self.last_seen = Some(target.eye);
            }
        } else if self.engaged {
            self.lost_for = self.lost_for.saturating_add(1);
            if self.lost_for > cfg.forget_ticks {
                self.engaged = false;
                self.last_seen = None;
            }
        }
    }

    /// The phase this tick: engaged bots chase or attack; disengaged bots patrol.
    pub(super) fn next_phase(
        &self,
        cfg: &BotConfig,
        visible: bool,
        distance: Option<f32>,
        nav: &Waypoints,
    ) -> Phase {
        if self.engaged {
            let in_engage_range = distance.is_some_and(|d| d <= cfg.engage_range);
            if visible && in_engage_range {
                Phase::Attack
            } else {
                Phase::Chase
            }
        } else {
            match self.phase {
                Phase::Patrol { waypoint } => Phase::Patrol { waypoint },
                Phase::Idle | Phase::Chase | Phase::Attack => {
                    if nav.is_empty() {
                        Phase::Idle
                    } else {
                        Phase::Patrol {
                            waypoint: WaypointId::FIRST,
                        }
                    }
                }
            }
        }
    }
}
