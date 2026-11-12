//! Client-side input history, authoritative correction, and visual error decay.
//!
//! Simulation is replayed from the authoritative state at its tick. Rendering
//! receives a separate scalar correction magnitude that decays on caller time.

use std::collections::VecDeque;

use engine_core::Tick;
use engine_world::{NetId, World};
use glam::Vec3;

use crate::error::NetError;

/// Game- or kit-specific simulation state and input behavior for one predicted entity.
pub trait Predicted {
    /// State corrected by the server.
    type State: Clone + PartialEq;
    /// Input replayed after an authoritative correction.
    type Input: Clone;

    /// Captures the state that must be restored or compared.
    fn capture(world: &World, id: NetId) -> Result<Self::State, NetError>;
    /// Restores an authoritative state.
    fn restore(world: &mut World, id: NetId, state: &Self::State) -> Result<(), NetError>;
    /// Advances one simulation tick with an input.
    fn step(world: &mut World, id: NetId, input: &Self::Input) -> Result<(), NetError>;
    /// World-space position used to measure and smooth corrections.
    fn position(state: &Self::State) -> Vec3;
}

/// Reconciliation tuning, expressed in deterministic tick units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PredictionConfig {
    /// Errors at or below this value do not trigger replay.
    pub epsilon: f32,
    /// Errors above this value snap without visual smoothing.
    pub snap_distance: f32,
    /// Visual correction decay duration in ticks.
    pub correction_ticks: u32,
}

impl Default for PredictionConfig {
    fn default() -> Self {
        Self {
            epsilon: 0.001,
            snap_distance: 2.0,
            correction_ticks: 6,
        }
    }
}

#[derive(Clone)]
struct Prediction<I, S> {
    tick: Tick,
    input: I,
    state: S,
}

/// Fixed-size prediction history for one client-owned entity.
pub struct PredictionHistory<P: Predicted> {
    capacity: usize,
    records: VecDeque<Prediction<P::Input, P::State>>,
    correction_start: Vec3,
    correction_elapsed: u32,
    config: PredictionConfig,
}

impl<P: Predicted> PredictionHistory<P> {
    /// Reserves a fixed ring for recent inputs and server predictions.
    #[must_use]
    pub fn with_capacity(capacity: usize, config: PredictionConfig) -> Self {
        Self {
            capacity,
            records: VecDeque::with_capacity(capacity),
            correction_start: Vec3::ZERO,
            correction_elapsed: 0,
            config,
        }
    }

    /// Position predicted for a command tick still held in the history ring.
    #[must_use]
    pub fn position_at(&self, tick: Tick) -> Option<Vec3> {
        self.records
            .iter()
            .find(|prediction| prediction.tick == tick)
            .map(|prediction| P::position(&prediction.state))
    }

    /// Steps locally and records the input and resulting state.
    ///
    /// # Errors
    /// Propagates state capture or game step errors.
    pub fn predict(
        &mut self,
        world: &mut World,
        id: NetId,
        tick: Tick,
        input: P::Input,
    ) -> Result<(), NetError> {
        P::step(world, id, &input)?;
        let state = P::capture(world, id)?;
        if self.records.len() == self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(Prediction { tick, input, state });
        Ok(())
    }

    /// Reconciles with an authoritative state and replays later stored inputs.
    ///
    /// # Errors
    /// Propagates state restore, capture, or replay errors.
    pub fn reconcile(
        &mut self,
        world: &mut World,
        id: NetId,
        tick: Tick,
        authoritative: &P::State,
    ) -> Result<bool, NetError> {
        let Some(prediction) = self.records.iter().find(|record| record.tick == tick) else {
            return Ok(false);
        };
        let predicted_position = P::position(&prediction.state);
        let corrected_position = P::position(authoritative);
        let correction = predicted_position - corrected_position;
        let error = correction.length();
        if error <= self.config.epsilon {
            self.discard_through(tick);
            return Ok(false);
        }
        P::restore(world, id, authoritative)?;
        self.discard_through(tick);
        for index in 0..self.records.len() {
            let Some(input) = self.records.get(index).map(|record| &record.input) else {
                continue;
            };
            P::step(world, id, input)?;
            let state = P::capture(world, id)?;
            if let Some(record) = self.records.get_mut(index) {
                record.state = state;
            }
        }
        if error > self.config.snap_distance {
            self.correction_start = Vec3::ZERO;
            self.correction_elapsed = self.config.correction_ticks;
        } else {
            self.correction_start = correction;
            self.correction_elapsed = 0;
        }
        Ok(true)
    }

    /// Advances correction decay by one deterministic simulation tick.
    pub fn advance_correction(&mut self) {
        if self.correction_elapsed < self.config.correction_ticks {
            self.correction_elapsed += 1;
        }
    }

    /// Visual render offset for the current tick plus fractional render alpha.
    #[must_use]
    pub fn offset_at(&self, alpha: f32) -> Vec3 {
        if self.config.correction_ticks == 0
            || self.correction_elapsed >= self.config.correction_ticks
        {
            return Vec3::ZERO;
        }
        let alpha = if alpha.is_finite() {
            alpha.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let elapsed = ticks_as_f32(self.correction_elapsed) + alpha;
        let duration = ticks_as_f32(self.config.correction_ticks);
        self.correction_start * (1.0 - (elapsed / duration).clamp(0.0, 1.0))
    }

    fn discard_through(&mut self, tick: Tick) {
        while self
            .records
            .front()
            .is_some_and(|record| record.tick == tick || tick.is_after(record.tick))
        {
            self.records.pop_front();
        }
    }
}

fn ticks_as_f32(ticks: u32) -> f32 {
    let high = u16::try_from(ticks >> 16).unwrap_or(u16::MAX);
    let low = u16::try_from(ticks & u32::from(u16::MAX)).unwrap_or(u16::MAX);
    f32::from(high) * 65_536.0 + f32::from(low)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use engine_world::World;

    struct Point;
    impl Predicted for Point {
        type State = Vec3;
        type Input = Vec3;
        fn capture(world: &World, id: NetId) -> Result<Vec3, NetError> {
            Ok(*world
                .get::<Vec3>(id)
                .map_err(|error| NetError::World(error.to_string()))?)
        }
        fn restore(world: &mut World, id: NetId, state: &Vec3) -> Result<(), NetError> {
            world
                .insert(id, *state)
                .map_err(|error| NetError::World(error.to_string()))
        }
        fn step(world: &mut World, id: NetId, input: &Vec3) -> Result<(), NetError> {
            let mut value = world
                .get_mut::<Vec3>(id)
                .map_err(|error| NetError::World(error.to_string()))?;
            *value += *input;
            Ok(())
        }
        fn position(state: &Vec3) -> Vec3 {
            *state
        }
    }

    #[test]
    fn authoritative_correction_replays_and_decays() {
        let mut world = World::new(1).unwrap();
        world
            .register::<Vec3>(engine_world::Replication::Replicated)
            .unwrap();
        let id = world.spawn().unwrap();
        world.insert(id, Vec3::ZERO).unwrap();
        let mut history = PredictionHistory::<Point>::with_capacity(8, PredictionConfig::default());
        history
            .predict(&mut world, id, Tick::new(1), Vec3::X)
            .unwrap();
        history
            .predict(&mut world, id, Tick::new(2), Vec3::X)
            .unwrap();
        assert!(
            history
                .reconcile(&mut world, id, Tick::new(1), &Vec3::splat(0.5))
                .unwrap()
        );
        assert_eq!(*world.get::<Vec3>(id).unwrap(), Vec3::new(1.5, 0.5, 0.5));
        assert_eq!(history.offset_at(0.0), Vec3::new(0.5, -0.5, -0.5));
        assert_eq!(
            history.offset_at(0.5),
            Vec3::new(0.458_333_34, -0.458_333_34, -0.458_333_34)
        );
        for _ in 0..6 {
            history.advance_correction();
        }
        assert_eq!(history.offset_at(0.0), Vec3::ZERO);
    }

    #[test]
    fn large_authoritative_error_snaps_without_visual_offset() {
        let mut world = World::new(2).unwrap();
        world
            .register::<Vec3>(engine_world::Replication::Replicated)
            .unwrap();
        let id = world.spawn().unwrap();
        world.insert(id, Vec3::ZERO).unwrap();
        let mut history = PredictionHistory::<Point>::with_capacity(4, PredictionConfig::default());
        history
            .predict(&mut world, id, Tick::new(1), Vec3::X)
            .unwrap();
        assert!(
            history
                .reconcile(&mut world, id, Tick::new(1), &Vec3::splat(-10.0))
                .unwrap()
        );
        assert_eq!(history.offset_at(0.0), Vec3::ZERO);
        assert_eq!(*world.get::<Vec3>(id).unwrap(), Vec3::splat(-10.0));
    }
}
