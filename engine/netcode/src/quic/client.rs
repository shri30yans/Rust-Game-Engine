//! The client endpoint: one connection to one server, pinned by certificate hash, used through
//! [`Transport`]. `connect` returns at once; [`QuicClient::state`] says when it is usable.
//!
//! [`Transport`]: crate::transport::Transport

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use super::identity::{CertificateHash, PinnedServer};
use super::io::{self, CLOSE_NORMAL, Command, Delivered, Delivery, IoShared, Link};
use super::{ALPN, ClientId, CloseReason, DEFAULT_IDLE_TIMEOUT, STREAM_PREFACE};
use crate::error::NetError;
use crate::protocol::Channel;
use crate::transport::Transport;

/// The one connection id a client uses internally for its server.
const SERVER: ClientId = ClientId::new(0, 0);
/// TLS server name; pinning by hash makes the name irrelevant, but TLS requires one.
const SERVER_NAME: &str = "engine-server";

/// How a client connects.
pub struct ClientConfig {
    /// The server's address.
    pub server: SocketAddr,
    /// The only certificate the client accepts.
    pub pinned: CertificateHash,
    /// Time without packets after which the connection is closed.
    pub idle_timeout: Duration,
}

impl ClientConfig {
    /// A config with the default idle timeout.
    #[must_use]
    pub const fn new(server: SocketAddr, pinned: CertificateHash) -> Self {
        Self {
            server,
            pinned,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }
}

/// Where the connection is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientState {
    /// The handshake is in progress; sends fail with [`NetError::NotConnected`].
    Connecting,
    /// The connection is open.
    Connected,
    /// The connection ended (or never opened) for this reason.
    Closed(CloseReason),
}

/// The client endpoint.
pub struct QuicClient {
    link: Link,
    state: ClientState,
}

impl QuicClient {
    /// Starts connecting in the background and returns at once.
    ///
    /// # Errors
    /// Returns [`NetError::QuicSetup`] when the local socket or TLS cannot be set up.
    pub fn connect(config: ClientConfig) -> Result<Self, NetError> {
        let quinn_config = client_config(config.pinned, config.idle_timeout)?;
        let local: SocketAddr = if config.server.is_ipv4() {
            (Ipv4Addr::UNSPECIFIED, 0).into()
        } else {
            (Ipv6Addr::UNSPECIFIED, 0).into()
        };
        let server = config.server;
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let link = Link::start("net-client", move |commands, shared| {
            let runtime = match io::runtime() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _caller_gone_is_fine = ready_tx.send(Err(error));
                    return;
                }
            };
            runtime.block_on(async move {
                let mut endpoint = match quinn::Endpoint::client(local) {
                    Ok(endpoint) => endpoint,
                    Err(error) => {
                        let _caller_gone_is_fine =
                            ready_tx.send(Err(NetError::QuicSetup(format!("socket: {error}"))));
                        return;
                    }
                };
                endpoint.set_default_client_config(quinn_config);
                let _caller_gone_is_fine = ready_tx.send(Ok(()));
                run(endpoint, server, commands, shared).await;
            });
        })?;
        ready_rx
            .recv()
            .map_err(|_| NetError::QuicSetup("network thread stopped during setup".to_owned()))??;
        Ok(Self {
            link,
            state: ClientState::Connecting,
        })
    }

    /// The connection state as of the last [`Transport::recv`] call.
    #[must_use]
    pub const fn state(&self) -> &ClientState {
        &self.state
    }

    /// Unreliable messages dropped because they were not drained fast enough.
    #[must_use]
    pub fn dropped_unreliable(&self) -> u64 {
        self.link.dropped_unreliable()
    }
}

impl Transport for QuicClient {
    /// # Errors
    /// [`NetError::NotConnected`] while connecting, [`NetError::ChannelClosed`] once closed,
    /// [`NetError::PayloadOverTransportLimit`] and [`NetError::SendQueueFull`] as for the server.
    fn send(&mut self, channel: Channel, bytes: &[u8]) -> Result<(), NetError> {
        match self.state {
            ClientState::Connecting => Err(NetError::NotConnected),
            ClientState::Closed(_) => Err(NetError::ChannelClosed),
            ClientState::Connected => self.link.send(SERVER, channel, bytes),
        }
    }

    /// Also advances [`QuicClient::state`]. Returns [`NetError::ChannelClosed`] once the
    /// connection is closed and every message that arrived before the close has been read.
    fn recv(&mut self, out: &mut Vec<u8>) -> Result<Option<Channel>, NetError> {
        while let Some(delivered) = self.link.next(out) {
            match delivered {
                Delivered::Connected(_) => self.state = ClientState::Connected,
                Delivered::Disconnected(_, reason) => self.state = ClientState::Closed(reason),
                Delivered::Message(_, channel) => return Ok(Some(channel)),
            }
        }
        match self.state {
            ClientState::Closed(_) => Err(NetError::ChannelClosed),
            ClientState::Connecting | ClientState::Connected => Ok(None),
        }
    }
}

/// QUIC transport settings shared by both ends: idle timeout, keep-alive at a third of it, and
/// datagrams enabled.
pub(super) fn transport_config(idle_timeout: Duration) -> Result<quinn::TransportConfig, NetError> {
    let idle = quinn::IdleTimeout::try_from(idle_timeout)
        .map_err(|error| NetError::QuicSetup(format!("idle timeout: {error}")))?;
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_idle_timeout(Some(idle))
        .keep_alive_interval(Some(idle_timeout / 3))
        .datagram_receive_buffer_size(Some(1 << 20));
    Ok(transport)
}

fn client_config(
    pinned: CertificateHash,
    idle_timeout: Duration,
) -> Result<quinn::ClientConfig, NetError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| NetError::QuicSetup(format!("tls: {error}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedServer::new(pinned, provider)))
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|error| NetError::QuicSetup(format!("quic tls: {error}")))?;
    let mut config = quinn::ClientConfig::new(Arc::new(crypto));
    config.transport_config(Arc::new(transport_config(idle_timeout)?));
    Ok(config)
}

/// Opens the connection and its reliable stream, then serves commands until shutdown.
async fn run(
    endpoint: quinn::Endpoint,
    server: SocketAddr,
    mut commands: mpsc::Receiver<Command>,
    shared: IoShared,
) {
    let connecting = async {
        let connection = endpoint
            .connect(server, SERVER_NAME)
            .map_err(|error| CloseReason::Transport(error.to_string()))?
            .await
            .map_err(|error| CloseReason::from_connection_error(&error))?;
        io::check_datagram_room(&connection)?;
        let (mut send, recv) = connection
            .open_bi()
            .await
            .map_err(|error| CloseReason::from_connection_error(&error))?;
        send.write_all(&[STREAM_PREFACE])
            .await
            .map_err(|error| CloseReason::Transport(error.to_string()))?;
        Ok::<_, CloseReason>((connection, send, recv))
    };
    // Only `Shutdown` can arrive while connecting (sends are refused on the game side), so a
    // command here ends the thread at once instead of waiting for the handshake to time out.
    let opened = tokio::select! {
        opened = connecting => opened,
        _ = commands.recv() => return,
    };
    let live = match opened {
        Ok((connection, send, recv)) => {
            shared.deliver_control(Delivery::Connected(SERVER));
            let watcher = shared.clone();
            Some(io::run_connection(
                SERVER,
                connection,
                send,
                recv,
                &shared,
                move |reason| watcher.deliver_control(Delivery::Disconnected(SERVER, reason)),
            ))
        }
        Err(reason) => {
            shared.deliver_control(Delivery::Disconnected(SERVER, reason));
            None
        }
    };
    while let Some(command) = commands.recv().await {
        match command {
            Command::Shutdown => break,
            Command::Send { channel, bytes, .. } => match &live {
                Some(link) => link.send(channel, bytes, &shared.pool),
                None => shared.pool.give(bytes),
            },
            Command::Disconnect(_) => {
                if let Some(link) = &live {
                    link.connection.close(CLOSE_NORMAL.into(), b"client left");
                }
            }
        }
    }
    if let Some(link) = &live {
        link.connection
            .close(CLOSE_NORMAL.into(), b"client shutting down");
    }
    let _idle_wait_timed_out_is_fine =
        tokio::time::timeout(Duration::from_secs(1), endpoint.wait_idle()).await;
}
