//! The real network transport: QUIC through quinn, behind the same [`Transport`] contract the
//! loopback uses, so replication code never sees a socket.
//!
//! [`QuicServer`] owns the socket and one connection per
//! client (named by [`ClientId`]); [`QuicClient`] is one connection and implements [`Transport`].
//!
//! Invariants:
//! - All network I/O runs on one dedicated thread with a small tokio runtime. The game thread only
//!   moves byte buffers through bounded queues: it never blocks, never awaits, and reuses buffers.
//! - [`Channel::Reliable`] is one long-lived bidirectional QUIC stream per connection, framed with a
//!   `u16` big-endian length; [`Channel::Unreliable`] is QUIC datagrams.
//! - The client trusts exactly one server certificate, named by its SHA-256 hash
//!   ([`CertificateHash`]), and still verifies the handshake signatures against it. There is no
//!   "accept any certificate" mode.
//!
//! [`Transport`]: crate::transport::Transport
//! [`Channel::Reliable`]: crate::protocol::Channel::Reliable
//! [`Channel::Unreliable`]: crate::protocol::Channel::Unreliable

mod client;
mod identity;
mod io;
mod server;

use std::time::Duration;

pub use crate::transport::{ClientId, CloseReason, ServerEvent};
pub use client::{ClientConfig, ClientState, QuicClient};
pub use identity::{CertificateHash, ServerIdentity};
pub use server::{QuicServer, ServerConfig};

/// ALPN protocol id: both ends must speak this engine's wire protocol.
const ALPN: &[u8] = b"engnet/2";
/// First byte a client writes on its reliable stream, so the server sees the stream at once.
const STREAM_PREFACE: u8 = 0xE7;
/// Largest reliable frame (the length prefix is a `u16`).
pub const MAX_RELIABLE_FRAME_BYTES: usize = u16::MAX as usize;
/// Default time without packets after which a connection is closed.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
/// Capacity of each queue between the game thread and the network thread.
const QUEUE_CAPACITY: usize = 1024;
/// Reliable frames queued per connection before the connection is closed as too slow.
const RELIABLE_BACKLOG: usize = 256;

impl CloseReason {
    fn from_connection_error(error: &quinn::ConnectionError) -> Self {
        match error {
            quinn::ConnectionError::TimedOut => Self::TimedOut,
            quinn::ConnectionError::ApplicationClosed(close) => {
                Self::ClosedByPeer(u32::try_from(close.error_code.into_inner()).unwrap_or(u32::MAX))
            }
            quinn::ConnectionError::LocallyClosed => Self::ClosedLocally,
            quinn::ConnectionError::ConnectionClosed(close)
                if close.error_code == quinn::TransportErrorCode::CONNECTION_REFUSED =>
            {
                Self::Refused
            }
            other => Self::Transport(other.to_string()),
        }
    }
}
