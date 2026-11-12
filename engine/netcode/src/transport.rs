//! The transport contracts. A client talks to one server through [`Transport`]; a server talks to
//! many clients through [`ServerTransport`], each named by a [`ClientId`]. Everything above these traits (handshake, lobby, replication) never
//! sees a socket, so the loopback and QUIC transports are interchangeable.

use crate::error::NetError;
use crate::protocol::Channel;

/// Sends and receives opaque datagram bytes on reliable or unreliable channels.
pub trait Transport {
    /// Queues one payload for delivery.
    ///
    /// # Errors
    /// Returns [`NetError::ChannelClosed`] when the peer is gone.
    fn send(&mut self, channel: Channel, bytes: &[u8]) -> Result<(), NetError>;

    /// Takes the next ready payload, writing it into `out` (cleared first).
    ///
    /// # Errors
    /// Propagates transport errors.
    fn recv(&mut self, out: &mut Vec<u8>) -> Result<Option<Channel>, NetError>;
}

/// A connection on the server: a slot index plus a generation, so a reconnect gets a new id and a
/// stale id can never address someone else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId {
    index: u32,
    generation: u32,
}

impl ClientId {
    /// An id from its parts; only transports make ids.
    pub(crate) const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Slot index (reused after a disconnect; compare whole ids).
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Generation of the slot when this connection took it.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// Why a connection ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason {
    /// The server was full or refused the connection.
    Refused,
    /// No packets arrived within the idle timeout.
    TimedOut,
    /// The peer closed the connection (application close code).
    ClosedByPeer(u32),
    /// This side closed it.
    ClosedLocally,
    /// The peer could not keep up with reliable traffic, or broke the stream framing.
    Protocol(&'static str),
    /// Any other transport or TLS failure, with the transport's description.
    Transport(String),
}

/// What happened on a server since the last poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    /// A client finished connecting and can be sent to.
    Connected(ClientId),
    /// A client's connection ended.
    Disconnected(ClientId, CloseReason),
    /// A message arrived from a client; its bytes are in the caller's buffer.
    Message(ClientId, Channel),
}

/// The server side of a transport: many clients, polled in arrival order.
///
/// Invariant: a client's `Connected` comes before any of its messages, and its `Disconnected`
/// after all of them; sending to a client after its `Disconnected` fails.
pub trait ServerTransport {
    /// Takes the next event. A message's bytes are written into `out` (cleared first).
    fn recv(&mut self, out: &mut Vec<u8>) -> Option<ServerEvent>;

    /// Queues `bytes` for `client`.
    ///
    /// # Errors
    /// [`NetError::ChannelClosed`] when `client` is not connected; transport-specific errors
    /// for oversized payloads or full queues.
    fn send(&mut self, client: ClientId, channel: Channel, bytes: &[u8]) -> Result<(), NetError>;

    /// Closes `client`'s connection; a `Disconnected` event follows.
    ///
    /// # Errors
    /// Transport-specific errors when the close cannot be queued.
    fn disconnect(&mut self, client: ClientId) -> Result<(), NetError>;
}
