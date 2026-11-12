//! Stuck detection for moving bots: a bot that covers too little ground for too long drops its enemy
//! and patrols again from a waypoint its own RNG picks.
//!
//! Invariants: only a moving bot counts progress; the progress window restarts after every check
//! that passes or fires; a stuck bot's new patrol waypoint is drawn from its own stream.

use engine_core::SimRng;

use super::brain::{BotBrain, BotInput, Phase};
use super::config::BotConfig;
use crate::error::KitError;
use crate::waypoints::{Waypoints, horizontal};

impl BotBrain {
    /// Watches a moving bot's progress. A bot that covers less than `stuck_progress` metres within
    /// `stuck_ticks` ticks is stuck: it drops its enemy and patrols from a waypoint its RNG picks.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] when the RNG cannot pick a waypoint.
    pub(super) fn watch_progress(
        &mut self,
        cfg: &BotConfig,
        input: &BotInput,
        nav: &Waypoints,
        rng: &mut SimRng,
    ) -> Result<(), KitError> {
        if !self.moving {
            self.anchor = input.eye;
            self.stalled_for = 0;
            return Ok(());
        }
        self.stalled_for = self.stalled_for.saturating_add(1);
        let moved = horizontal(input.eye - self.anchor).length();
        if moved >= cfg.stuck_progress {
            self.anchor = input.eye;
            self.stalled_for = 0;
            return Ok(());
        }
        if self.stalled_for < cfg.stuck_ticks {
            return Ok(());
        }
        self.anchor = input.eye;
        self.stalled_for = 0;
        self.engaged = false;
        self.last_seen = None;
        self.phase = match nav.pick(rng)? {
            Some(waypoint) => Phase::Patrol { waypoint },
            None => Phase::Idle,
        };
        Ok(())
    }
}
