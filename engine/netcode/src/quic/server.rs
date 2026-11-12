//! The server endpoint: one UDP socket, up to `max_clients` connections, each named by a
//! [`ClientId`]. The game polls [`QuicServer::recv`] each frame for connects, disconnects and
//! messages in arrival order, and sends with [`QuicServer::send`].

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use super::identity::{CertificateHash, ServerIdentity};
use super::io::{
    self, CLOSE_NORMAL, CLOSE_PROTOCOL, Command, Delivered, Delivery, IoShared, Link,
    LiveConnection,
};
use super::{ALPN, DEFAULT_IDLE_TIMEOUT, STREAM_PREFACE};
use crate::error::NetError;
use crate::protocol::Channel;
use crate::transport::{ClientId, CloseReason, ServerEvent, ServerTransport};

/// How a server listens.
pub struct ServerConfig {
    /// Address and port to bind (port 0 picks a free port).
    pub bind: SocketAddr,
    /// Connections beyond this count are refused.
    pub max_clients: u32,
    /// Time without packets after which a connection is closed.
    pub idle_timeout: Duration,
    /// The certificate and key the server presents.
    pub identity: ServerIdentity,
}

impl ServerConfig {
    /// A config with the default idle timeout.
    #[must_use]
    pub fn new(bind: SocketAddr, max_clients: u32, identity: ServerIdentity) -> Self {
        Self {
            bind,
            max_clients,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            identity,
        }
    }
}

/// The server endpoint.
pub struct QuicServer {
    link: Link,
    local_addr: SocketAddr,
    certificate_hash: CertificateHash,
    /// Live generation per slot, as the game has been told (send to anything else fails).
    live: Vec<Option<u32>>,
}

impl QuicServer {
    /// Binds the socket and starts the network thread.
    ///
    /// # Errors
    /// Returns [`NetError::QuicSetup`] when the address cannot be bound or TLS cannot be set up.
    pub fn bind(config: ServerConfig) -> Result<Self, NetError> {
        let certificate_hash = config.identity.certificate_hash();
        let quinn_config = server_config(config.identity, config.idle_timeout)?;
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let bind = config.bind;
        let max_clients = config.max_clients;
        let link = Link::start("net-server", move |commands, shared| {
            let runtime = match io::runtime() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _caller_gone_is_fine = ready_tx.send(Err(error));
                    return;
                }
            };
            runtime.block_on(async move {
                let endpoint = match quinn::Endpoint::server(quinn_config, bind) {
                    Ok(endpoint) => endpoint,
                    Err(error) => {
                        let _caller_gone_is_fine = ready_tx
                            .send(Err(NetError::QuicSetup(format!("bind {bind}: {error}"))));
                        return;
                    }
                };
                let local = endpoint
                    .local_addr()
                    .map_err(|error| NetError::QuicSetup(format!("local address: {error}")));
                let _caller_gone_is_fine = ready_tx.send(local);
                serve(endpoint, commands, shared, max_clients).await;
            });
        })?;
        let local_addr = ready_rx
            .recv()
            .map_err(|_| NetError::QuicSetup("network thread stopped during bind".to_owned()))??;
        Ok(Self {
            link,
            local_addr,
            certificate_hash,
            live: Vec::new(),
        })
    }

    /// The bound address (with the real port when the config asked for port 0).
    #[must_use]
    pub const fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// The hash clients must pin to connect.
    #[must_use]
    pub const fn certificate_hash(&self) -> CertificateHash {
        self.certificate_hash
    }

    /// Unreliable messages dropped because events were not drained fast enough.
    #[must_use]
    pub fn dropped_unreliable(&self) -> u64 {
        self.link.dropped_unreliable()
    }

    fn is_live(&self, client: ClientId) -> bool {
        usize::try_from(client.index())
            .ok()
            .and_then(|index| self.live.get(index))
            .is_some_and(|generation| *generation == Some(client.generation()))
    }

    fn set_live(&mut self, client: ClientId, live: bool) {
        let Ok(index) = usize::try_from(client.index()) else {
            return;
        };
        if index >= self.live.len() {
            self.live.resize(index + 1, None);
        }
        if let Some(slot) = self.live.get_mut(index) {
            if live {
                *slot = Some(client.generation());
            } else if *slot == Some(client.generation()) {
                *slot = None;
            }
        }
    }
}

impl ServerTransport for QuicServer {
    fn recv(&mut self, out: &mut Vec<u8>) -> Option<ServerEvent> {
        Some(match self.link.next(out)? {
            Delivered::Connected(client) => {
                self.set_live(client, true);
                ServerEvent::Connected(client)
            }
            Delivered::Disconnected(client, reason) => {
                self.set_live(client, false);
                ServerEvent::Disconnected(client, reason)
            }
            Delivered::Message(client, channel) => ServerEvent::Message(client, channel),
        })
    }

    fn send(&mut self, client: ClientId, channel: Channel, bytes: &[u8]) -> Result<(), NetError> {
        if !self.is_live(client) {
            return Err(NetError::ChannelClosed);
        }
        self.link.send(client, channel, bytes)
    }

    fn disconnect(&mut self, client: ClientId) -> Result<(), NetError> {
        self.link.disconnect(client)
    }
}

fn server_config(
    identity: ServerIdentity,
    idle_timeout: Duration,
) -> Result<quinn::ServerConfig, NetError> {
    let (certificate, key) = identity.into_parts();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut tls = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| NetError::QuicSetup(format!("tls: {error}")))?
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)
        .map_err(|error| NetError::QuicSetup(format!("tls certificate: {error}")))?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls)
        .map_err(|error| NetError::QuicSetup(format!("quic tls: {error}")))?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(Arc::new(super::client::transport_config(idle_timeout)?));
    Ok(config)
}

/// Slot table owned by the network thread: which connections exist and their generations.
struct Slots {
    connections: Vec<SlotState>,
    active: u32,
}

enum SlotState {
    Free {
        generation: u32,
    },
    Handshaking {
        generation: u32,
    },
    Live {
        generation: u32,
        link: LiveConnection,
    },
}

impl Slots {
    fn reserve(&mut self) -> ClientId {
        self.active += 1;
        for (index, slot) in self.connections.iter_mut().enumerate() {
            if let SlotState::Free { generation } = *slot {
                let generation = generation.wrapping_add(1);
                *slot = SlotState::Handshaking { generation };
                return ClientId::new(u32::try_from(index).unwrap_or(u32::MAX), generation);
            }
        }
        let index = u32::try_from(self.connections.len()).unwrap_or(u32::MAX);
        self.connections
            .push(SlotState::Handshaking { generation: 0 });
        ClientId::new(index, 0)
    }

    fn slot(&mut self, client: ClientId) -> Option<&mut SlotState> {
        let slot = self
            .connections
            .get_mut(usize::try_from(client.index()).ok()?)?;
        let generation = match slot {
            SlotState::Free { .. } => return None,
            SlotState::Handshaking { generation } | SlotState::Live { generation, .. } => {
                *generation
            }
        };
        (generation == client.generation()).then_some(slot)
    }

    fn live(&mut self, client: ClientId) -> Option<&LiveConnection> {
        match self.slot(client)? {
            SlotState::Live { link, .. } => Some(link),
            _ => None,
        }
    }

    fn release(&mut self, client: ClientId) {
        if let Some(slot) = self.slot(client) {
            *slot = SlotState::Free {
                generation: client.generation(),
            };
            self.active = self.active.saturating_sub(1);
        }
    }
}

/// What connection tasks tell the main loop, on one ordered channel. The main loop is the only
/// place that announces connects and disconnects to the game, so the game always sees `Connected`
/// before `Disconnected`, and never a `Connected` for a slot the network thread cannot send to.
enum SlotEvent {
    Ready(ClientId, LiveConnection),
    Failed(ClientId),
    Closed(ClientId, CloseReason),
}

async fn serve(
    endpoint: quinn::Endpoint,
    mut commands: mpsc::Receiver<Command>,
    shared: IoShared,
    max_clients: u32,
) {
    let mut slots = Slots {
        connections: Vec::new(),
        active: 0,
    };
    let (events_tx, mut events) = mpsc::unbounded_channel();
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                None | Some(Command::Shutdown) => break,
                Some(Command::Send { client, channel, bytes }) => match slots.live(client) {
                    Some(link) => link.send(channel, bytes, &shared.pool),
                    None => shared.pool.give(bytes),
                },
                Some(Command::Disconnect(client)) => {
                    if let Some(link) = slots.live(client) {
                        link.connection.close(CLOSE_NORMAL.into(), b"disconnected by server");
                    }
                }
            },
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { break };
                if slots.active >= max_clients {
                    incoming.refuse();
                    continue;
                }
                let client = slots.reserve();
                tokio::spawn(handshake(client, incoming, shared.clone(), events_tx.clone()));
            },
            Some(event) = events.recv() => match event {
                SlotEvent::Ready(client, link) => {
                    if let Some(slot) = slots.slot(client) {
                        *slot = SlotState::Live { generation: client.generation(), link };
                        shared.deliver_control(Delivery::Connected(client));
                    }
                }
                SlotEvent::Failed(client) => slots.release(client),
                SlotEvent::Closed(client, reason) => {
                    if matches!(slots.slot(client), Some(SlotState::Live { .. })) {
                        shared.deliver_control(Delivery::Disconnected(client, reason));
                    }
                    slots.release(client);
                }
            },
        }
    }
    endpoint.close(CLOSE_NORMAL.into(), b"server shutting down");
    let _idle_wait_timed_out_is_fine =
        tokio::time::timeout(Duration::from_secs(1), endpoint.wait_idle()).await;
}

/// Completes one connection: TLS, the datagram room check and the client's reliable stream.
async fn handshake(
    client: ClientId,
    incoming: quinn::Incoming,
    shared: IoShared,
    events: mpsc::UnboundedSender<SlotEvent>,
) {
    let result = async {
        let connection = incoming.await.ok()?;
        if io::check_datagram_room(&connection).is_err() {
            connection.close(CLOSE_PROTOCOL.into(), b"datagrams too small");
            return None;
        }
        let (send, mut recv) = connection.accept_bi().await.ok()?;
        let mut preface = [0_u8; 1];
        recv.read_exact(&mut preface).await.ok()?;
        if preface[0] != STREAM_PREFACE {
            connection.close(CLOSE_PROTOCOL.into(), b"bad stream preface");
            return None;
        }
        Some((connection, send, recv))
    }
    .await;
    // No await between starting the connection's tasks and sending `Ready`: on this single-thread
    // runtime the watcher cannot run first, so `Ready` always precedes `Closed` on the channel.
    let event = match result {
        Some((connection, send, recv)) => {
            let closed = events.clone();
            let link = io::run_connection(client, connection, send, recv, &shared, move |reason| {
                let _main_loop_gone_is_fine = closed.send(SlotEvent::Closed(client, reason));
            });
            SlotEvent::Ready(client, link)
        }
        None => SlotEvent::Failed(client),
    };
    let _main_loop_gone_is_fine = events.send(event);
}
