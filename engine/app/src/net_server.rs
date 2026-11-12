//! Deterministic server-side session, room and match driver.
//!
//! The server owns the authoritative [`App`], one lobby host and one input/snapshot stream per
//! transport connection. Simulation advances only when [`NetServer::tick`] is called; wall-clock
//! pacing is isolated to [`run_dedicated`].

use std::thread;
use std::time::{Duration, Instant};

use engine_core::Tick;
use engine_netcode::{
    Channel, ClientId, EntityState, InputFrame, InputReceiver, InputRepeater, Message,
    NetCodecRegistry, ServerEvent, ServerHandshake, ServerTransport, SnapshotPacket,
    SnapshotWriter, TransformBounds, capture_world_into, decode, encode, encode_snapshot,
};
use engine_sessions::{
    LobbyHost, LobbyReply, LobbyRequest, Occupant, Phase, RoomConfig, RoomView, SeatId, TokenSource,
};
use engine_world::{NetId, World};

use crate::app::App;
use crate::clock::TickRate;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::extract::entity_capacity;
use crate::game::{NetGame, PlayerId};
use platform::DeviceProfile;

const DEFAULT_MAX_COMPONENT_BYTES: usize = 1024;
const SNAPSHOT_ENTITY_CAPACITY: usize = 4096;

struct Peer<I: engine_netcode::InputPayload> {
    id: ClientId,
    handshake: ServerHandshake,
    accepted: bool,
    input_receiver: InputReceiver,
    input_repeater: InputRepeater<I>,
    inputs: Vec<InputFrame<I>>,
    snapshot_writer: SnapshotWriter,
    packet: SnapshotPacket,
    possessed: Option<(SeatId, NetId)>,
}

impl<I: engine_netcode::InputPayload> Peer<I> {
    fn new(id: ClientId, protocol: engine_netcode::GameProtocol) -> Self {
        Self {
            id,
            handshake: ServerHandshake::new(protocol, 1),
            accepted: false,
            input_receiver: InputReceiver::new(),
            input_repeater: InputRepeater::new(),
            inputs: Vec::with_capacity(engine_netcode::INPUT_HISTORY),
            snapshot_writer: SnapshotWriter::with_capacity(SNAPSHOT_ENTITY_CAPACITY, 256),
            packet: SnapshotPacket::with_capacity(
                SNAPSHOT_ENTITY_CAPACITY,
                256,
                DEFAULT_MAX_COMPONENT_BYTES,
            ),
            possessed: None,
        }
    }
}

/// The result of one deterministic server tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerTick {
    /// Tick simulated by this call.
    pub tick: Tick,
    /// Whether the room is currently running a match.
    pub in_match: bool,
}

/// An authoritative game server driven by transport events and explicit fixed ticks.
pub struct NetServer<G, T, K>
where
    G: NetGame,
    T: ServerTransport,
    K: TokenSource,
{
    app: App<G>,
    transport: T,
    lobby: LobbyHost<ClientId, G::Settings, K>,
    peers: Vec<Peer<G::Command>>,
    pawns: Vec<(SeatId, NetId)>,
    recv_buffer: Vec<u8>,
    send_buffer: Vec<u8>,
    session_buffer: Vec<u8>,
    states: Vec<EntityState>,
    capture_buffer: Vec<u8>,
    codecs: NetCodecRegistry,
    bounds: TransformBounds,
    lobby_outbox: Vec<(ClientId, LobbyReply<G::Settings>)>,
    seat_scratch: Vec<SeatId>,
    max_component_bytes: usize,
}

impl<G, T, K> NetServer<G, T, K>
where
    G: NetGame,
    T: ServerTransport,
    K: TokenSource,
{
    /// Builds the authoritative app and one-room lobby around any server transport.
    pub fn new(
        game: G,
        config: &AppConfig,
        profile: &DeviceProfile,
        room_config: RoomConfig,
        settings: G::Settings,
        transport: T,
        tokens: K,
    ) -> Result<Self, AppError> {
        let bounds = game.transform_bounds(&settings)?;
        let app = App::new(game, &settings, config, profile)?;
        let capacity = entity_capacity(app.world(), app.game().dynamic_entity_budget())?;
        Ok(Self {
            lobby: LobbyHost::new(engine_sessions::Room::new(room_config, settings), tokens),
            app,
            transport,
            peers: Vec::with_capacity(usize::from(room_config.max_seats())),
            pawns: Vec::with_capacity(usize::from(room_config.max_seats())),
            recv_buffer: Vec::with_capacity(engine_netcode::MAX_MESSAGE_BYTES),
            send_buffer: Vec::with_capacity(engine_netcode::MAX_MESSAGE_BYTES),
            session_buffer: Vec::with_capacity(engine_netcode::MAX_SESSION_BYTES),
            states: Vec::with_capacity(capacity),
            capture_buffer: Vec::with_capacity(DEFAULT_MAX_COMPONENT_BYTES),
            codecs: NetCodecRegistry::default(),
            bounds,
            lobby_outbox: Vec::with_capacity(usize::from(room_config.max_seats()) * 2),
            seat_scratch: Vec::with_capacity(usize::from(room_config.max_seats())),
            max_component_bytes: DEFAULT_MAX_COMPONENT_BYTES,
        })
    }

    /// The current authoritative world.
    #[must_use]
    pub const fn world(&self) -> &World {
        self.app.world()
    }

    /// The current simulation tick.
    #[must_use]
    pub const fn tick_number(&self) -> Tick {
        self.app.tick()
    }

    /// The token-free lobby view.
    #[must_use]
    pub fn lobby(&self) -> RoomView<G::Settings> {
        self.lobby.room().view()
    }

    /// The server transport, for an outer event loop to advance a deterministic transport.
    pub const fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Advances transport, room, input, simulation and snapshots by one fixed tick.
    pub fn tick(&mut self) -> Result<ServerTick, AppError> {
        let tick = self.app.tick();
        self.drain_transport(tick)?;
        let previous_phase = self.lobby.room().phase();
        self.lobby.tick(tick, &mut self.lobby_outbox);
        if !matches!(previous_phase, Phase::InMatch) && self.lobby.room().phase() == Phase::InMatch
        {
            self.travel_to_match()?;
        }
        self.flush_lobby_replies()?;
        self.sync_pawns()?;
        self.apply_inputs(tick)?;
        self.app.step()?;
        let in_match = matches!(self.lobby.room().phase(), engine_sessions::Phase::InMatch);
        if in_match {
            self.write_snapshots(tick)?;
        }
        Ok(ServerTick { tick, in_match })
    }

    fn travel_to_match(&mut self) -> Result<(), AppError> {
        let settings = self.lobby.room().settings();
        let bounds = self.app.game().transform_bounds(settings)?;
        self.app.travel(settings)?;
        self.bounds = bounds;
        self.pawns.clear();
        for peer in &mut self.peers {
            peer.possessed = None;
        }
        Ok(())
    }

    /// Ends the current match and publishes the results phase to the lobby.
    pub fn end_match(&mut self) -> Result<(), AppError> {
        self.lobby.end_match(&mut self.lobby_outbox)?;
        self.flush_lobby_replies()?;
        Ok(())
    }

    /// Returns a results room to the lobby and removes pawns whose seats were not resumed.
    pub fn return_to_lobby(&mut self) -> Result<(), AppError> {
        self.lobby.return_to_lobby(&mut self.lobby_outbox)?;
        self.flush_lobby_replies()?;
        self.sync_pawns()
    }

    fn drain_transport(&mut self, tick: Tick) -> Result<(), AppError> {
        while let Some(event) = self.transport.recv(&mut self.recv_buffer) {
            match event {
                ServerEvent::Connected(id) => {
                    self.peers.push(Peer::new(id, self.game().protocol()));
                }
                ServerEvent::Disconnected(id, _) => {
                    self.lobby.on_disconnect(id, &mut self.lobby_outbox);
                    self.peers.retain(|peer| peer.id != id);
                    self.flush_lobby_replies()?;
                }
                ServerEvent::Message(id, channel) => {
                    let message = decode::<G::Command>(&self.recv_buffer)?;
                    if message.channel() != channel {
                        self.transport.disconnect(id)?;
                        continue;
                    }
                    self.handle_message(id, message, tick)?;
                }
            }
        }
        Ok(())
    }

    fn handle_message(
        &mut self,
        id: ClientId,
        message: Message<G::Command>,
        tick: Tick,
    ) -> Result<(), AppError> {
        match message {
            Message::Hello {
                version,
                game,
                client_nonce,
            } => {
                let Some(peer) = self.peer_mut(id) else {
                    return Ok(());
                };
                let response: Message<G::Command> =
                    peer.handshake.on_hello(version, game, client_nonce);
                peer.accepted = matches!(response, Message::Welcome { .. });
                self.send_message(id, &response, Channel::Reliable)?;
            }
            Message::Session(bytes) => {
                if !self.peer(id).is_some_and(|peer| peer.accepted) {
                    self.transport.disconnect(id)?;
                    return Ok(());
                }
                let request: LobbyRequest<G::Settings> =
                    postcard::from_bytes(&bytes).map_err(engine_netcode::NetError::Decode)?;
                if matches!(request, LobbyRequest::Start)
                    && self
                        .lobby
                        .room()
                        .host()
                        .is_some_and(|host| self.lobby.conn_of(host) == Some(id))
                {
                    let bot_count = self
                        .lobby
                        .room()
                        .seats()
                        .iter()
                        .filter(|seat| matches!(seat.occupant(), Occupant::Bot))
                        .count();
                    let settings = self
                        .app
                        .game()
                        .settings_for_match(self.lobby.room().settings(), bot_count)?;
                    if settings != *self.lobby.room().settings() {
                        self.lobby.on_request(
                            id,
                            LobbyRequest::SetSettings(settings),
                            tick,
                            &mut self.lobby_outbox,
                        );
                        self.flush_lobby_replies()?;
                    }
                }
                self.lobby
                    .on_request(id, request, tick, &mut self.lobby_outbox);
                self.flush_lobby_replies()?;
                self.possess_peer(id)?;
            }
            Message::Inputs(bundle) => {
                let Some(peer) = self.peer_mut(id).filter(|peer| peer.accepted) else {
                    return Ok(());
                };
                peer.snapshot_writer.acknowledge(bundle.snapshot_ack());
                for frame in peer.input_receiver.accept(&bundle) {
                    if peer.inputs.len() == engine_netcode::INPUT_HISTORY {
                        peer.inputs.remove(0);
                    }
                    peer.inputs.push(frame);
                }
            }
            Message::Reject { .. }
            | Message::Welcome { .. }
            | Message::Possess { .. }
            | Message::Snapshot(_) => {}
        }
        Ok(())
    }

    fn flush_lobby_replies(&mut self) -> Result<(), AppError> {
        while !self.lobby_outbox.is_empty() {
            let (client, reply) = self.lobby_outbox.remove(0);
            self.session_buffer.clear();
            self.session_buffer =
                postcard::to_extend(&reply, core::mem::take(&mut self.session_buffer))
                    .map_err(engine_netcode::NetError::Decode)?;
            let message = Message::<G::Command>::Session(self.session_buffer.clone());
            self.send_message(client, &message, Channel::Reliable)?;
        }
        Ok(())
    }

    fn sync_pawns(&mut self) -> Result<(), AppError> {
        if !matches!(
            self.lobby.room().phase(),
            engine_sessions::Phase::InMatch | engine_sessions::Phase::Results
        ) {
            return Ok(());
        }
        self.seat_scratch.clear();
        self.seat_scratch
            .extend(self.lobby.room().seats().iter().map(|seat| seat.id()));
        for index in 0..self.seat_scratch.len() {
            let Some(seat) = self.seat_scratch.get(index).copied() else {
                continue;
            };
            if !self.pawns.iter().any(|(known, _)| *known == seat) {
                let Some(name) = self.lobby.room().seat(seat).map(|seat| seat.name().clone())
                else {
                    continue;
                };
                let pawn = self.app.join_player(PlayerId::from_seat(seat), &name)?;
                self.pawns.push((seat, pawn));
            }
            if let Some(client) = self.lobby.conn_of(seat) {
                self.possess_peer(client)?;
            }
        }
        let mut index = 0;
        while index < self.pawns.len() {
            let Some((seat, _)) = self.pawns.get(index).copied() else {
                break;
            };
            if self.lobby.room().seat(seat).is_none() {
                self.app.leave_player(PlayerId::from_seat(seat))?;
                self.pawns.swap_remove(index);
            } else {
                index += 1;
            }
        }
        Ok(())
    }

    fn possess_peer(&mut self, client: ClientId) -> Result<(), AppError> {
        let Some(seat) = self.lobby.seat_of(client) else {
            return Ok(());
        };
        let Some((_, pawn)) = self.pawns.iter().find(|(known, _)| *known == seat).copied() else {
            return Ok(());
        };
        let Some(peer) = self.peer_mut(client) else {
            return Ok(());
        };
        if peer.possessed == Some((seat, pawn)) {
            return Ok(());
        }
        peer.possessed = Some((seat, pawn));
        self.send_message(
            client,
            &Message::<G::Command>::Possess { pawn },
            Channel::Reliable,
        )
    }

    fn apply_inputs(&mut self, tick: Tick) -> Result<(), AppError> {
        for peer_index in 0..self.peers.len() {
            let Some(peer) = self.peers.get_mut(peer_index) else {
                continue;
            };
            if !peer.accepted {
                continue;
            }
            let Some(seat) = self.lobby.seat_of(peer.id) else {
                continue;
            };
            let Some((_, command)) = Self::take_input(peer, tick) else {
                continue;
            };
            self.app
                .apply_player_command(PlayerId::from_seat(seat), &command)?;
        }
        Ok(())
    }

    fn take_input(peer: &mut Peer<G::Command>, tick: Tick) -> Option<(Tick, G::Command)> {
        let mut current = None;
        peer.inputs.retain(|frame| {
            if frame.tick() == tick {
                current = Some(frame.payload().clone());
                false
            } else {
                frame.tick().is_after(tick)
            }
        });
        peer.input_repeater
            .for_tick(current)
            .map(|command| (tick, command))
    }

    fn write_snapshots(&mut self, tick: Tick) -> Result<(), AppError> {
        capture_world_into(
            self.app.world(),
            None,
            None,
            self.bounds,
            &self.codecs,
            &mut self.states,
            &mut self.capture_buffer,
        )?;
        for peer in &mut self.peers {
            let Some((seat, _)) = peer.possessed else {
                continue;
            };
            let player = PlayerId::from_seat(seat);
            let game = self.app.game();
            peer.snapshot_writer
                .reserve_component_bytes(self.max_component_bytes);
            peer.snapshot_writer.write_into_with_policy(
                tick,
                &self.states,
                &mut peer.packet,
                |entity| game.replication(self.app.world(), player, entity),
            )?;
            peer.packet.set_travel_epoch(self.app.travel_epoch());
            encode_snapshot(&peer.packet, &mut self.send_buffer)?;
            self.transport
                .send(peer.id, Channel::Unreliable, &self.send_buffer)?;
        }
        Ok(())
    }

    fn send_message<I: engine_netcode::InputPayload>(
        &mut self,
        client: ClientId,
        message: &Message<I>,
        channel: Channel,
    ) -> Result<(), AppError> {
        encode(message, &mut self.send_buffer)?;
        self.transport.send(client, channel, &self.send_buffer)?;
        Ok(())
    }

    fn peer(&self, id: ClientId) -> Option<&Peer<G::Command>> {
        self.peers.iter().find(|peer| peer.id == id)
    }

    fn peer_mut(&mut self, id: ClientId) -> Option<&mut Peer<G::Command>> {
        self.peers.iter_mut().find(|peer| peer.id == id)
    }

    fn game(&self) -> &G {
        self.app.game()
    }
}

/// Runs a server at a fixed tick rate on the calling thread. This function is the sole place where
/// a server runner reads the wall clock; simulation itself remains deterministic.
pub fn run_dedicated<G, T, K>(
    server: &mut NetServer<G, T, K>,
    tick_rate: TickRate,
) -> Result<(), AppError>
where
    G: NetGame,
    T: ServerTransport,
    K: TokenSource,
{
    let interval = Duration::from_nanos(tick_rate.step_nanos());
    let mut deadline = Instant::now();
    loop {
        deadline += interval;
        server.tick()?;
        let now = Instant::now();
        if now < deadline {
            thread::sleep(deadline - now);
        } else {
            eprintln!("dedicated server tick overrun: {:?}", now - deadline);
        }
    }
}
