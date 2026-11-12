//! Ordered, named systems and the fixed-step driver that runs them.
//!
//! Systems run in insertion order, once per tick, with a [`SystemContext`] that
//! carries the tick, the fixed timestep and a private RNG stream. Each stream is
//! forked from the world's root RNG using a hash of the system's name, so adding
//! or reordering other systems never changes a system's random sequence.

use engine_core::{SimRng, StateHasher, Tick};

use crate::error::WorldError;
use crate::world::World;

/// Per-tick data handed to every system.
pub struct SystemContext<'a> {
    /// Tick being simulated.
    pub tick: Tick,
    /// Fixed timestep in seconds. Constant for the life of the schedule.
    pub dt: f32,
    /// This system's private RNG stream. Persists across ticks.
    pub rng: &'a mut SimRng,
}

/// Signature of a schedulable system.
pub type SystemFn = fn(&mut World, &mut SystemContext<'_>) -> Result<(), WorldError>;

struct SystemEntry {
    name: &'static str,
    run: SystemFn,
    rng: SimRng,
}

/// Ordered list of systems plus the fixed timestep they run at.
pub struct Schedule {
    dt: f32,
    root: SimRng,
    systems: Vec<SystemEntry>,
}

impl Schedule {
    /// Creates an empty schedule. Each system's RNG is forked from `root`.
    ///
    /// # Errors
    /// Returns [`WorldError::InvalidDt`] when `dt` is not finite and positive.
    pub fn new(root: &SimRng, dt: f32) -> Result<Self, WorldError> {
        if !dt.is_finite() || dt <= 0.0 {
            return Err(WorldError::InvalidDt(dt));
        }
        Ok(Self {
            dt,
            root: root.clone(),
            systems: Vec::new(),
        })
    }

    /// Appends a system. It runs after every system added before it.
    ///
    /// # Errors
    /// Returns [`WorldError::DuplicateSystem`] if `name` is already scheduled.
    pub fn add_system(&mut self, name: &'static str, run: SystemFn) -> Result<(), WorldError> {
        if self.systems.iter().any(|entry| entry.name == name) {
            return Err(WorldError::DuplicateSystem(name));
        }
        let mut hasher = StateHasher::new();
        hasher.write_bytes(name.as_bytes());
        let rng = self.root.fork(hasher.finish());
        self.systems.push(SystemEntry { name, run, rng });
        Ok(())
    }

    /// Names of the scheduled systems in run order.
    pub fn system_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.systems.iter().map(|entry| entry.name)
    }

    /// Fixed timestep in seconds.
    #[must_use]
    pub const fn dt(&self) -> f32 {
        self.dt
    }

    /// Runs every system once, in order, for `tick`.
    ///
    /// # Errors
    /// Stops at the first failing system and returns it as
    /// [`WorldError::System`]. Systems before it have already run.
    pub fn run_tick(&mut self, world: &mut World, tick: Tick) -> Result<(), WorldError> {
        let dt = self.dt;
        for entry in &mut self.systems {
            let mut ctx = SystemContext {
                tick,
                dt,
                rng: &mut entry.rng,
            };
            (entry.run)(world, &mut ctx).map_err(|source| WorldError::System {
                system: entry.name,
                source: Box::new(source),
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn noop(_: &mut World, _: &mut SystemContext<'_>) -> Result<(), WorldError> {
        Ok(())
    }

    fn fail(_: &mut World, _: &mut SystemContext<'_>) -> Result<(), WorldError> {
        Err(WorldError::Invariant("boom"))
    }

    #[test]
    fn rejects_bad_dt_and_duplicate_names() {
        let root = SimRng::new(1, 0);
        assert!(Schedule::new(&root, 0.0).is_err());
        assert!(Schedule::new(&root, f32::NAN).is_err());
        let mut schedule = Schedule::new(&root, 1.0 / 60.0).unwrap();
        schedule.add_system("a", noop).unwrap();
        assert!(matches!(
            schedule.add_system("a", noop),
            Err(WorldError::DuplicateSystem("a"))
        ));
    }

    #[test]
    fn systems_run_in_insertion_order_and_failure_is_named() {
        let mut world = World::new(1).unwrap();
        let root = SimRng::new(1, 0);
        let mut schedule = Schedule::new(&root, 1.0 / 60.0).unwrap();
        schedule.add_system("ok", noop).unwrap();
        schedule.add_system("bad", fail).unwrap();
        let names: Vec<_> = schedule.system_names().collect();
        assert_eq!(names, ["ok", "bad"]);
        let err = schedule.run_tick(&mut world, Tick::ZERO).unwrap_err();
        assert!(matches!(err, WorldError::System { system: "bad", .. }));
    }
}
