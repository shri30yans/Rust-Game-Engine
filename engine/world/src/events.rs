//! Per-tick typed event logs: how systems tell each other what happened in a tick.
//!
//! A game or kit registers each event type once with [`World::add_events`](crate::World::add_events),
//! sizing its buffer from data. Writers append with [`World::send`](crate::World::send); readers
//! iterate [`Events`] through [`World::events`](crate::World::events). Reading never consumes, so
//! any number of readers see the same events in any order. The [`Schedule`](crate::Schedule)
//! clears every registered log once, before the first system of a tick; no system clears a log.
//!
//! Invariants: one log per event type per world; a log holds exactly the events written since the
//! start of the current tick, in write order; clearing keeps each buffer's capacity, so a tick that
//! stays within the registered capacity allocates nothing. Logs are not part of the state hash:
//! they are emptied before the next tick runs, so they cannot carry state from one tick to the next.

use std::any::{Any, type_name};

use crate::error::WorldError;

/// The events of one type written during the current tick, in write order.
#[derive(Debug)]
pub struct Events<E> {
    items: Vec<E>,
}

impl<E> Events<E> {
    /// An empty log with room for `capacity` events before it grows.
    fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }

    /// The events written this tick, in write order.
    #[must_use]
    pub fn as_slice(&self) -> &[E] {
        &self.items
    }

    /// Iterates the events written this tick, in write order.
    pub fn iter(&self) -> std::slice::Iter<'_, E> {
        self.items.iter()
    }

    /// The event at `index` in write order, if that many were written.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&E> {
        self.items.get(index)
    }

    /// Number of events written this tick.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no event was written this tick.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Events the log holds before it has to grow.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.items.capacity()
    }

    /// Appends one event. Past the registered capacity the buffer grows, so no event is lost.
    fn push(&mut self, event: E) {
        self.items.push(event);
    }
}

impl<'a, E> IntoIterator for &'a Events<E> {
    type Item = &'a E;
    type IntoIter = std::slice::Iter<'a, E>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

/// A type-erased log the store can clear without knowing its event type.
trait EventLog: Any + Send + Sync {
    /// Empties the log, keeping its capacity.
    fn clear(&mut self);
}

impl<E: Send + Sync + 'static> EventLog for Events<E> {
    fn clear(&mut self) {
        self.items.clear();
    }
}

/// Private storage for the event logs of one world.
#[derive(Default)]
pub(crate) struct EventStore {
    logs: Vec<Box<dyn EventLog>>,
}

impl EventStore {
    /// Registers a log for `E` with room for `capacity` events.
    pub(crate) fn add<E: Send + Sync + 'static>(
        &mut self,
        capacity: usize,
    ) -> Result<(), WorldError> {
        if self.find::<E>().is_some() {
            return Err(WorldError::DuplicateEvents(type_name::<E>()));
        }
        self.logs
            .push(Box::new(Events::<E>::with_capacity(capacity)));
        Ok(())
    }

    /// The log for `E`.
    pub(crate) fn get<E: Send + Sync + 'static>(&self) -> Result<&Events<E>, WorldError> {
        self.find::<E>()
            .ok_or(WorldError::MissingEvents(type_name::<E>()))
    }

    /// Appends `event` to the log for `E`.
    pub(crate) fn send<E: Send + Sync + 'static>(&mut self, event: E) -> Result<(), WorldError> {
        self.logs
            .iter_mut()
            .find_map(|log| {
                let any: &mut dyn Any = &mut **log;
                any.downcast_mut::<Events<E>>()
            })
            .ok_or(WorldError::MissingEvents(type_name::<E>()))?
            .push(event);
        Ok(())
    }

    /// Empties every log, keeping each buffer's capacity.
    pub(crate) fn clear_all(&mut self) {
        for log in &mut self.logs {
            log.clear();
        }
    }

    fn find<E: Send + Sync + 'static>(&self) -> Option<&Events<E>> {
        self.logs.iter().find_map(|log| {
            let any: &dyn Any = &**log;
            any.downcast_ref::<Events<E>>()
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use engine_core::{SimRng, Tick};

    use crate::{Schedule, SystemContext, World, WorldError};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Ping(u32);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Pong;

    fn write_two(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
        world.send(Ping(ctx.tick.raw()))?;
        world.send(Ping(ctx.tick.raw() + 100))
    }

    fn read_and_echo(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
        let count = world.events::<Ping>()?.len();
        for _ in 0..count {
            world.send(Pong)?;
        }
        Ok(())
    }

    #[test]
    fn a_type_registers_once_and_an_unregistered_type_is_named() {
        let mut world = World::new(1).unwrap();
        world.add_events::<Ping>(4).unwrap();
        assert!(matches!(
            world.add_events::<Ping>(4),
            Err(WorldError::DuplicateEvents(_))
        ));
        assert!(matches!(
            world.send(Pong),
            Err(WorldError::MissingEvents(_))
        ));
        assert!(matches!(
            world.events::<Pong>(),
            Err(WorldError::MissingEvents(_))
        ));
    }

    #[test]
    fn capacity_comes_from_registration() {
        let mut world = World::new(1).unwrap();
        world.add_events::<Ping>(24).unwrap();
        assert!(world.events::<Ping>().unwrap().capacity() >= 24);
        assert!(world.events::<Ping>().unwrap().is_empty());
    }

    #[test]
    fn the_schedule_clears_every_log_once_before_the_first_system() {
        let mut world = World::new(1).unwrap();
        world.add_events::<Ping>(4).unwrap();
        world.add_events::<Pong>(4).unwrap();
        let mut schedule = Schedule::new(&SimRng::new(1, 0), 1.0 / 60.0).unwrap();
        schedule.add_system("write", write_two).unwrap();
        schedule.add_system("echo", read_and_echo).unwrap();
        // An event written between ticks is visible until the next tick starts.
        world.send(Ping(999)).unwrap();
        for tick in 0..3 {
            schedule.run_tick(&mut world, Tick::new(tick)).unwrap();
            let pings: Vec<_> = world.events::<Ping>().unwrap().iter().copied().collect();
            assert_eq!(pings, [Ping(tick), Ping(tick + 100)]);
            assert_eq!(world.events::<Pong>().unwrap().len(), 2);
        }
    }

    #[test]
    fn reading_never_consumes_so_every_reader_sees_the_same_events() {
        let mut world = World::new(1).unwrap();
        world.add_events::<Ping>(4).unwrap();
        world.send(Ping(1)).unwrap();
        world.send(Ping(2)).unwrap();
        let first: Vec<_> = world.events::<Ping>().unwrap().iter().copied().collect();
        let mut second = Vec::new();
        for ping in world.events::<Ping>().unwrap() {
            second.push(*ping);
        }
        assert_eq!(first, second);
        assert_eq!(world.events::<Ping>().unwrap().get(1), Some(&Ping(2)));
        assert_eq!(world.events::<Ping>().unwrap().as_slice().len(), 2);
    }

    #[test]
    fn logs_stay_out_of_the_state_hash() {
        let mut quiet = World::new(1).unwrap();
        quiet.add_events::<Ping>(4).unwrap();
        let mut noisy = World::new(1).unwrap();
        noisy.add_events::<Ping>(4).unwrap();
        noisy.send(Ping(7)).unwrap();
        assert_eq!(quiet.state_hash().unwrap(), noisy.state_hash().unwrap());
    }
}
