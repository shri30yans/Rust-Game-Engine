//! The list of rooms a server (or a matchmaking service) knows about, so players can find one
//! by code or by browsing. Each entry is a small public listing, refreshed by heartbeats and dropped when they stop.
//!
//! Invariants: codes are unique; entries iterate in code order, so browsing is deterministic;
//! an entry whose last heartbeat is `ttl` ticks old or more is gone after [`SessionDirectory::expire`].

use std::collections::BTreeMap;

use engine_core::{SimRng, Tick};

use crate::codes::RoomCode;
use crate::error::SessionError;

/// Draws before giving up on finding a free code. With 2^30 codes, failing this many times
/// means the directory is absurdly full or the generator is broken.
const CODE_ATTEMPTS: u32 = 64;

/// One listed room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry<L> {
    /// The game's public listing (mode, map, player count, region...).
    pub listing: L,
    /// The tick of the last heartbeat.
    pub last_seen: Tick,
}

/// Rooms by code. `L` is the game's listing type.
#[derive(Debug, Clone)]
pub struct SessionDirectory<L> {
    rooms: BTreeMap<RoomCode, DirectoryEntry<L>>,
    rng: SimRng,
    ttl: u32,
}

impl<L> SessionDirectory<L> {
    /// An empty directory. `rng` draws room codes (seed it from OS entropy on a real server so
    /// codes differ across restarts); entries live `ttl` ticks past their last heartbeat.
    #[must_use]
    pub const fn new(rng: SimRng, ttl: u32) -> Self {
        Self {
            rooms: BTreeMap::new(),
            rng,
            ttl,
        }
    }

    /// Lists a room under a fresh code.
    ///
    /// # Errors
    /// [`SessionError::CodesExhausted`] when no free code turns up.
    pub fn register(&mut self, listing: L, now: Tick) -> Result<RoomCode, SessionError> {
        for _ in 0..CODE_ATTEMPTS {
            let code = RoomCode::generate(&mut self.rng);
            if let std::collections::btree_map::Entry::Vacant(slot) = self.rooms.entry(code) {
                slot.insert(DirectoryEntry {
                    listing,
                    last_seen: now,
                });
                return Ok(code);
            }
        }
        Err(SessionError::CodesExhausted)
    }

    /// Refreshes a room's listing and keeps it alive.
    ///
    /// # Errors
    /// [`SessionError::UnknownRoom`].
    pub fn heartbeat(&mut self, code: RoomCode, listing: L, now: Tick) -> Result<(), SessionError> {
        let entry = self.rooms.get_mut(&code).ok_or(SessionError::UnknownRoom)?;
        entry.listing = listing;
        entry.last_seen = now;
        Ok(())
    }

    /// Unlists a room.
    ///
    /// # Errors
    /// [`SessionError::UnknownRoom`].
    pub fn remove(&mut self, code: RoomCode) -> Result<L, SessionError> {
        self.rooms
            .remove(&code)
            .map(|entry| entry.listing)
            .ok_or(SessionError::UnknownRoom)
    }

    /// Finds a room by code.
    #[must_use]
    pub fn lookup(&self, code: RoomCode) -> Option<&DirectoryEntry<L>> {
        self.rooms.get(&code)
    }

    /// Rooms whose listing passes `filter`, in code order.
    pub fn browse<'a>(
        &'a self,
        mut filter: impl FnMut(&L) -> bool + 'a,
    ) -> impl Iterator<Item = (RoomCode, &'a L)> + 'a {
        self.rooms
            .iter()
            .filter(move |(_, entry)| filter(&entry.listing))
            .map(|(code, entry)| (*code, &entry.listing))
    }

    /// Drops rooms whose last heartbeat is `ttl` ticks old or more, returning how many.
    pub fn expire(&mut self, now: Tick) -> usize {
        let before = self.rooms.len();
        let ttl = self.ttl;
        self.rooms
            .retain(|_, entry| now.is_before(entry.last_seen.advance(ttl)));
        before - self.rooms.len()
    }

    /// Listed rooms.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rooms.len()
    }

    /// Whether no room is listed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rooms.is_empty()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn rooms_are_found_by_code_browsed_in_order_and_expire_without_heartbeats() {
        let mut directory = SessionDirectory::new(SimRng::new(7, 0), 100);
        let a = directory.register(4_u8, Tick::ZERO).unwrap();
        let b = directory.register(9_u8, Tick::new(50)).unwrap();
        assert_ne!(a, b);
        assert_eq!(directory.lookup(a).map(|entry| entry.listing), Some(4));

        let open: Vec<_> = directory.browse(|players| *players < 8).collect();
        assert_eq!(open, vec![(a, &4)]);
        let all: Vec<_> = directory.browse(|_| true).map(|(code, _)| code).collect();
        assert!(all.is_sorted());

        assert_eq!(directory.expire(Tick::new(99)), 0);
        assert_eq!(directory.expire(Tick::new(100)), 1);
        assert!(directory.lookup(a).is_none());
        directory.heartbeat(b, 10, Tick::new(140)).unwrap();
        assert_eq!(directory.expire(Tick::new(200)), 0);
        assert_eq!(
            directory.heartbeat(a, 1, Tick::new(200)),
            Err(SessionError::UnknownRoom)
        );
        assert_eq!(directory.remove(b), Ok(10));
        assert!(directory.is_empty());
    }

    #[test]
    fn many_registrations_get_unique_codes() {
        let mut directory = SessionDirectory::new(SimRng::new(1, 2), 10);
        for n in 0..5_000_u32 {
            directory.register(n, Tick::ZERO).unwrap();
        }
        assert_eq!(directory.len(), 5_000);
    }
}
