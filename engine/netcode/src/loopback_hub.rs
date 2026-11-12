//! The server side of many loopback links: an in-process [`ServerTransport`] for deterministic
//! tests of a server with several clients, with the same link model as [`loopback_pair`].
//!
//! Invariants: `Connected` for a client is reported before any of its messages and
//! `Disconnected` after all of them; slots are reused with a new generation, so a stale
//! [`ClientId`] never reaches a new client; event order depends only on the calls made, the
//! seeds and the ticks, never on timing.

use std::collections::VecDeque;

use engine_core::Tick;

use crate::error::NetError;
use crate::loopback::{LinkConditions, LoopbackEnd, loopback_pair};
use crate::protocol::Channel;
use crate::transport::{ClientId, CloseReason, ServerEvent, ServerTransport, Transport};

/// One live link: the server's end of it.
struct Link {
    id: ClientId,
    end: LoopbackEnd,
}

/// Many loopback clients behind one server transport.
pub struct LoopbackHub {
    slots: Vec<Option<Link>>,
    generations: Vec<u32>,
    max_clients: usize,
    events: VecDeque<ServerEvent>,
    /// Next slot to read a message from, so no client starves the others.
    cursor: usize,
    now: Tick,
    seed: u64,
}

impl LoopbackHub {
    /// A hub that accepts up to `max_clients` at once. `seed` makes every link's loss, jitter
    /// and reorder reproducible.
    #[must_use]
    pub fn new(max_clients: usize, seed: u64) -> Self {
        Self {
            slots: Vec::with_capacity(max_clients),
            generations: Vec::with_capacity(max_clients),
            max_clients,
            events: VecDeque::with_capacity(max_clients * 2),
            cursor: 0,
            now: Tick::ZERO,
            seed,
        }
    }

    /// Opens a link and returns the client's end; the server sees `Connected` on its next
    /// [`ServerTransport::recv`]. `to_server` and `to_client` model each direction.
    ///
    /// # Errors
    /// [`NetError::ServerFull`] when `max_clients` links are open.
    pub fn connect(
        &mut self,
        to_server: LinkConditions,
        to_client: LinkConditions,
    ) -> Result<LoopbackEnd, NetError> {
        let free = self.slots.iter().position(Option::is_none);
        let index = match free {
            Some(index) => index,
            None if self.slots.len() < self.max_clients => {
                self.slots.push(None);
                self.generations.push(0);
                self.slots.len() - 1
            }
            None => return Err(NetError::ServerFull),
        };
        let generation = self.generations.get(index).copied().unwrap_or(0);
        let id = ClientId::new(
            u32::try_from(index).map_err(|_| NetError::ServerFull)?,
            generation,
        );
        let link_seed = self.seed ^ (u64::from(id.index()) << 32 | u64::from(generation));
        let (mut client, mut server) = loopback_pair(to_server, to_client, link_seed);
        client.advance_to(self.now);
        server.advance_to(self.now);
        if let Some(slot) = self.slots.get_mut(index) {
            *slot = Some(Link { id, end: server });
        }
        self.events.push_back(ServerEvent::Connected(id));
        Ok(client)
    }

    /// Moves every link's clock to `now`, releasing packets due by then.
    pub fn advance_to(&mut self, now: Tick) {
        self.now = now;
        for link in self.slots.iter_mut().flatten() {
            link.end.advance_to(now);
        }
    }

    /// Clients connected now.
    #[must_use]
    pub fn connected(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    fn link_mut(&mut self, client: ClientId) -> Option<&mut Link> {
        let index = usize::try_from(client.index()).ok()?;
        self.slots
            .get_mut(index)?
            .as_mut()
            .filter(|link| link.id == client)
    }

    /// Frees a slot, bumping its generation, and reports the close.
    fn close(&mut self, index: usize, reason: CloseReason) {
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        if let Some(link) = slot.take() {
            if let Some(generation) = self.generations.get_mut(index) {
                *generation = generation.wrapping_add(1);
            }
            self.events
                .push_back(ServerEvent::Disconnected(link.id, reason));
        }
    }
}

impl ServerTransport for LoopbackHub {
    fn recv(&mut self, out: &mut Vec<u8>) -> Option<ServerEvent> {
        if let Some(event) = self.events.pop_front() {
            return Some(event);
        }
        let count = self.slots.len();
        for step in 0..count {
            let index = (self.cursor + step) % count;
            let Some(link) = self.slots.get_mut(index).and_then(Option::as_mut) else {
                continue;
            };
            match link.end.recv(out) {
                Ok(Some(channel)) => {
                    self.cursor = (index + 1) % count;
                    return Some(ServerEvent::Message(link.id, channel));
                }
                Ok(None) => {}
                Err(_) => {
                    self.close(index, CloseReason::ClosedByPeer(0));
                    return self.events.pop_front();
                }
            }
        }
        None
    }

    fn send(&mut self, client: ClientId, channel: Channel, bytes: &[u8]) -> Result<(), NetError> {
        self.link_mut(client)
            .ok_or(NetError::ChannelClosed)?
            .end
            .send(channel, bytes)
    }

    fn disconnect(&mut self, client: ClientId) -> Result<(), NetError> {
        if self.link_mut(client).is_none() {
            return Err(NetError::ChannelClosed);
        }
        let index = usize::try_from(client.index()).map_err(|_| NetError::ChannelClosed)?;
        self.close(index, CloseReason::ClosedLocally);
        Ok(())
    }
}
