//! The network thread's plumbing shared by server and client: the queues between the game thread
//! and the network thread, a buffer pool both sides reuse, and the tasks that run one connection.
//!
//! Each connection has four tasks: a writer that owns the reliable send stream (so one slow peer
//! cannot stall the others), a reliable-stream reader, a datagram reader, and a watcher that
//! reports the close reason exactly once.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use super::{ClientId, CloseReason, MAX_RELIABLE_FRAME_BYTES, QUEUE_CAPACITY, RELIABLE_BACKLOG};
use crate::error::NetError;
use crate::protocol::{Channel, MAX_DATAGRAM_BYTES};

/// Application close code: the peer did not keep up or broke the framing.
pub(super) const CLOSE_PROTOCOL: u32 = 1;
/// Application close code: an orderly close or shutdown.
pub(super) const CLOSE_NORMAL: u32 = 0;

/// A command from the game thread to the network thread.
pub(super) enum Command {
    Send {
        client: ClientId,
        channel: Channel,
        bytes: Vec<u8>,
    },
    Disconnect(ClientId),
    Shutdown,
}

/// Something the network thread hands to the game thread, in arrival order.
pub(super) enum Delivery {
    Connected(ClientId),
    Disconnected(ClientId, CloseReason),
    Message(ClientId, Channel, Vec<u8>),
}

/// Byte buffers reused in both directions so a steady stream of packets does not allocate.
#[derive(Clone, Default)]
pub(super) struct BufferPool(Arc<Mutex<Vec<Vec<u8>>>>);

impl BufferPool {
    pub(super) fn take(&self) -> Vec<u8> {
        self.0
            .lock()
            .ok()
            .and_then(|mut free| free.pop())
            .unwrap_or_else(|| Vec::with_capacity(MAX_DATAGRAM_BYTES))
    }

    pub(super) fn give(&self, mut buffer: Vec<u8>) {
        buffer.clear();
        if let Ok(mut free) = self.0.lock()
            && free.len() < QUEUE_CAPACITY
        {
            free.push(buffer);
        }
    }
}

/// What every connection task needs from the network thread.
///
/// Deliveries go through one queue so the game sees them in arrival order. The queue itself never
/// refuses (so connects and disconnects are never lost, and the network thread never blocks on the
/// game, which would deadlock a game thread waiting in `Drop`); messages are limited instead by
/// `queued_messages`, which the game side decrements as it reads them.
#[derive(Clone)]
pub(super) struct IoShared {
    pub(super) deliveries: Sender<Delivery>,
    pub(super) queued_messages: Arc<AtomicUsize>,
    pub(super) pool: BufferPool,
    pub(super) dropped_unreliable: Arc<AtomicU64>,
}

impl IoShared {
    /// Hands a received message to the game. A full queue drops an unreliable message (counted)
    /// and reports `false` for a reliable one, which the caller treats as a protocol failure.
    fn deliver_message(&self, client: ClientId, channel: Channel, bytes: Vec<u8>) -> bool {
        if self.queued_messages.load(Ordering::Acquire) >= QUEUE_CAPACITY {
            self.pool.give(bytes);
            if channel == Channel::Unreliable {
                self.dropped_unreliable.fetch_add(1, Ordering::Relaxed);
                return true;
            }
            return false;
        }
        self.queued_messages.fetch_add(1, Ordering::AcqRel);
        // A closed game side means the endpoint is being dropped; nothing is left to deliver to.
        self.deliveries
            .send(Delivery::Message(client, channel, bytes))
            .is_ok()
    }

    /// Hands a connect or disconnect to the game. Never refused and never blocks.
    pub(super) fn deliver_control(&self, delivery: Delivery) {
        let _closed_game_side_is_fine = self.deliveries.send(delivery);
    }
}

/// The game thread's end of the network thread.
pub(super) struct Link {
    commands: mpsc::Sender<Command>,
    deliveries: Receiver<Delivery>,
    queued_messages: Arc<AtomicUsize>,
    pool: BufferPool,
    dropped_unreliable: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl Link {
    /// Starts the network thread running `body`, which receives the commands and the shared I/O
    /// handles. The body owns its tokio runtime's lifetime: when it returns, the thread ends.
    pub(super) fn start<F>(name: &str, body: F) -> Result<Self, NetError>
    where
        F: FnOnce(mpsc::Receiver<Command>, IoShared) + Send + 'static,
    {
        let (command_tx, command_rx) = mpsc::channel(QUEUE_CAPACITY);
        let (delivery_tx, delivery_rx) = std::sync::mpsc::channel();
        let queued_messages = Arc::new(AtomicUsize::new(0));
        let pool = BufferPool::default();
        let dropped_unreliable = Arc::new(AtomicU64::new(0));
        let shared = IoShared {
            deliveries: delivery_tx,
            queued_messages: Arc::clone(&queued_messages),
            pool: pool.clone(),
            dropped_unreliable: Arc::clone(&dropped_unreliable),
        };
        let thread = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || body(command_rx, shared))
            .map_err(|error| NetError::QuicSetup(format!("network thread: {error}")))?;
        Ok(Self {
            commands: command_tx,
            deliveries: delivery_rx,
            queued_messages,
            pool,
            dropped_unreliable,
            thread: Some(thread),
        })
    }

    /// Queues `bytes` for `client` without blocking.
    pub(super) fn send(
        &self,
        client: ClientId,
        channel: Channel,
        bytes: &[u8],
    ) -> Result<(), NetError> {
        let cap = match channel {
            Channel::Unreliable => MAX_DATAGRAM_BYTES,
            Channel::Reliable => MAX_RELIABLE_FRAME_BYTES,
        };
        if bytes.len() > cap {
            return Err(NetError::PayloadOverTransportLimit {
                channel,
                size: bytes.len(),
                cap,
            });
        }
        let mut buffer = self.pool.take();
        buffer.extend_from_slice(bytes);
        self.commands
            .try_send(Command::Send {
                client,
                channel,
                bytes: buffer,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => NetError::SendQueueFull,
                mpsc::error::TrySendError::Closed(_) => NetError::ChannelClosed,
            })
    }

    /// Asks the network thread to close `client`'s connection.
    pub(super) fn disconnect(&self, client: ClientId) -> Result<(), NetError> {
        self.commands
            .try_send(Command::Disconnect(client))
            .map_err(|_| NetError::SendQueueFull)
    }

    /// Takes the next delivery; a message's bytes are copied into `out` and its buffer reused.
    pub(super) fn next(&self, out: &mut Vec<u8>) -> Option<Delivered> {
        let delivery = self.deliveries.try_recv().ok()?;
        Some(match delivery {
            Delivery::Connected(client) => Delivered::Connected(client),
            Delivery::Disconnected(client, reason) => Delivered::Disconnected(client, reason),
            Delivery::Message(client, channel, bytes) => {
                self.queued_messages.fetch_sub(1, Ordering::AcqRel);
                out.clear();
                out.extend_from_slice(&bytes);
                self.pool.give(bytes);
                Delivered::Message(client, channel)
            }
        })
    }

    /// Unreliable messages dropped because the game did not drain the queue fast enough.
    pub(super) fn dropped_unreliable(&self) -> u64 {
        self.dropped_unreliable.load(Ordering::Relaxed)
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        let _already_stopped_is_fine = self.commands.blocking_send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _a_panicked_io_thread_has_nothing_left_to_clean = thread.join();
        }
    }
}

/// A delivery as the game thread sees it (message bytes already copied out).
pub(super) enum Delivered {
    Connected(ClientId),
    Disconnected(ClientId, CloseReason),
    Message(ClientId, Channel),
}

/// Builds the network thread's runtime: one thread, timers and sockets enabled.
pub(super) fn runtime() -> Result<tokio::runtime::Runtime, NetError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| NetError::QuicSetup(format!("runtime: {error}")))
}

/// A connection the network thread can write to.
pub(super) struct LiveConnection {
    pub(super) connection: quinn::Connection,
    pub(super) reliable: mpsc::Sender<Vec<u8>>,
}

impl LiveConnection {
    /// Sends on the right channel; a reliable backlog that is full closes the connection, since
    /// the peer is not keeping up.
    pub(super) fn send(&self, channel: Channel, bytes: Vec<u8>, pool: &BufferPool) {
        match channel {
            Channel::Unreliable => {
                let datagram = bytes::Bytes::copy_from_slice(&bytes);
                pool.give(bytes);
                // A datagram that cannot be sent is lost, which is what unreliable means.
                let _lost_is_allowed = self.connection.send_datagram(datagram);
            }
            Channel::Reliable => {
                if let Err(mpsc::error::TrySendError::Full(bytes)) = self.reliable.try_send(bytes) {
                    pool.give(bytes);
                    self.connection
                        .close(CLOSE_PROTOCOL.into(), b"reliable backlog full");
                }
            }
        }
    }
}

/// Starts the four tasks of one connection and returns its writable handle. `on_closed` runs once,
/// with the reason, when the connection ends.
pub(super) fn run_connection(
    client: ClientId,
    connection: quinn::Connection,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    shared: &IoShared,
    on_closed: impl FnOnce(CloseReason) + Send + 'static,
) -> LiveConnection {
    let (reliable_tx, reliable_rx) = mpsc::channel(RELIABLE_BACKLOG);
    tokio::spawn(write_reliable(send, reliable_rx, shared.pool.clone()));
    tokio::spawn(read_reliable(
        client,
        connection.clone(),
        recv,
        shared.clone(),
    ));
    tokio::spawn(read_datagrams(client, connection.clone(), shared.clone()));
    let watched = connection.clone();
    tokio::spawn(async move {
        let error = watched.closed().await;
        on_closed(CloseReason::from_connection_error(&error));
    });
    LiveConnection {
        connection,
        reliable: reliable_tx,
    }
}

async fn write_reliable(
    mut send: quinn::SendStream,
    mut frames: mpsc::Receiver<Vec<u8>>,
    pool: BufferPool,
) {
    while let Some(frame) = frames.recv().await {
        let Ok(length) = u16::try_from(frame.len()) else {
            pool.give(frame);
            continue;
        };
        let written = async {
            send.write_all(&length.to_be_bytes()).await?;
            send.write_all(&frame).await
        }
        .await;
        pool.give(frame);
        if written.is_err() {
            return;
        }
    }
}

async fn read_reliable(
    client: ClientId,
    connection: quinn::Connection,
    mut recv: quinn::RecvStream,
    shared: IoShared,
) {
    loop {
        let mut length = [0_u8; 2];
        if recv.read_exact(&mut length).await.is_err() {
            return;
        }
        let mut frame = shared.pool.take();
        frame.resize(usize::from(u16::from_be_bytes(length)), 0);
        if recv.read_exact(&mut frame).await.is_err() {
            shared.pool.give(frame);
            return;
        }
        if !shared.deliver_message(client, Channel::Reliable, frame) {
            connection.close(CLOSE_PROTOCOL.into(), b"receive queue full");
            return;
        }
    }
}

async fn read_datagrams(client: ClientId, connection: quinn::Connection, shared: IoShared) {
    while let Ok(datagram) = connection.read_datagram().await {
        if datagram.len() > MAX_DATAGRAM_BYTES {
            continue;
        }
        let mut buffer = shared.pool.take();
        buffer.extend_from_slice(&datagram);
        shared.deliver_message(client, Channel::Unreliable, buffer);
    }
}

/// Fails a connection whose datagram limit is below the engine's packet cap: snapshots would not
/// fit, so it is refused clearly at connect time instead of silently losing every packet.
pub(super) fn check_datagram_room(connection: &quinn::Connection) -> Result<(), CloseReason> {
    match connection.max_datagram_size() {
        Some(size) if size >= MAX_DATAGRAM_BYTES => Ok(()),
        _ => Err(CloseReason::Protocol(
            "path cannot carry engine-sized datagrams",
        )),
    }
}
