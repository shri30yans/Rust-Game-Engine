//! Client-side handshake, lobby requests, prediction and remote interpolation.
//!
//! The client sends only validated commands and applies authoritative snapshots to replicated
//! entities. It never runs authoritative game rules.

use engine_core::Tick;
use engine_netcode::{
    ClientHandshake, GameProtocol, InputFrame, InputHistory, InterpolationConfig, Message,
    NetCodecRegistry, NetError, Predicted, PredictionConfig, PredictionHistory, SnapshotBuffer,
    SnapshotPacket, TransformBounds, Transport, apply_entity, decode, decode_transform, encode,
};
use engine_sessions::{LobbyReply, LobbyRequest, Membership, ResumeToken, RoomView};
use engine_world::{NetId, Transform, World};
use glam::Vec3;
use std::collections::VecDeque;

use crate::error::AppError;
use crate::game::NetGame;

const CLIENT_ENTITY_CAPACITY: usize = 4096;
const SNAPSHOT_STATES_PER_ENTITY: usize = 8;
const CONFIRMED_PREDICTION_HISTORY: usize = 128;
/// Input arrives this many ticks ahead to absorb the expected one-way network delay.
const INPUT_LEAD_TICKS: u32 = 6;

/// A client connected to a game server over any transport.
pub struct NetClient<G: NetGame, T: Transport> {
    transport: T,
    handshake: ClientHandshake,
    connected: bool,
    room: Option<RoomView<G::Settings>>,
    pending_travel: Option<G::Settings>,
    travel_epoch: u32,
    membership: Option<Membership>,
    resume_token: Option<ResumeToken>,
    pawn: Option<NetId>,
    tick: Tick,
    latest_server_tick: Option<Tick>,
    last_predicted_tick: Option<Tick>,
    input_history: InputHistory<G::Command>,
    prediction: PredictionHistory<G::Predicted>,
    confirmed_predictions: VecDeque<(Tick, Vec3)>,
    snapshots: SnapshotBuffer,
    replicated_entities: Vec<NetId>,
    interpolation: InterpolationConfig,
    recv_buffer: Vec<u8>,
    send_buffer: Vec<u8>,
    bounds: TransformBounds,
    pending_bounds: Option<TransformBounds>,
    codecs: NetCodecRegistry,
    _game: core::marker::PhantomData<G>,
}

impl<G: NetGame, T: Transport> NetClient<G, T> {
    /// Starts a handshake using this game's protocol identity.
    pub fn connect(
        mut transport: T,
        protocol: GameProtocol,
        game: &G,
        settings: &G::Settings,
    ) -> Result<Self, AppError> {
        let bounds = game.transform_bounds(settings)?;
        let (handshake, hello) = ClientHandshake::start::<G::Command>(protocol, 0);
        let mut send_buffer = Vec::with_capacity(engine_netcode::MAX_MESSAGE_BYTES);
        encode(&hello, &mut send_buffer)?;
        transport.send(engine_netcode::Channel::Reliable, &send_buffer)?;
        Ok(Self {
            transport,
            handshake,
            connected: false,
            room: None,
            pending_travel: None,
            travel_epoch: 0,
            membership: None,
            resume_token: None,
            pawn: None,
            tick: Tick::ZERO,
            latest_server_tick: None,
            last_predicted_tick: None,
            input_history: InputHistory::new(),
            prediction: PredictionHistory::with_capacity(128, PredictionConfig::default()),
            confirmed_predictions: VecDeque::with_capacity(CONFIRMED_PREDICTION_HISTORY),
            snapshots: SnapshotBuffer::with_capacity(
                CLIENT_ENTITY_CAPACITY,
                SNAPSHOT_STATES_PER_ENTITY,
            ),
            replicated_entities: Vec::with_capacity(CLIENT_ENTITY_CAPACITY),
            interpolation: InterpolationConfig::default(),
            recv_buffer: Vec::with_capacity(engine_netcode::MAX_MESSAGE_BYTES),
            send_buffer,
            bounds,
            pending_bounds: None,
            codecs: NetCodecRegistry::default(),
            _game: core::marker::PhantomData,
        })
    }

    /// Latest token-free room view, when a lobby reply has arrived.
    #[must_use]
    pub fn lobby(&self) -> Option<&RoomView<G::Settings>> {
        self.room.as_ref()
    }

    /// This connection's current seat or waiting place.
    #[must_use]
    pub const fn membership(&self) -> Option<Membership> {
        self.membership
    }

    /// The secret to send in a later lobby hello after reconnecting.
    #[must_use]
    pub const fn resume_token(&self) -> Option<ResumeToken> {
        self.resume_token
    }

    /// The pawn the server has assigned, after a reliable `Possess` message.
    #[must_use]
    pub const fn pawn(&self) -> Option<NetId> {
        self.pawn
    }

    /// Settings from a lobby transition into a match that require a client world travel.
    pub fn take_pending_travel(&mut self) -> Option<G::Settings> {
        self.pending_travel.take()
    }

    /// Advances the expected epoch after the client world has traveled.
    pub fn set_travel_epoch(&mut self, travel_epoch: u32) {
        if let Some(bounds) = self.pending_bounds.take() {
            self.bounds = bounds;
        }
        self.travel_epoch = travel_epoch;
        self.replicated_entities.clear();
        self.snapshots =
            SnapshotBuffer::with_capacity(CLIENT_ENTITY_CAPACITY, SNAPSHOT_STATES_PER_ENTITY);
        self.prediction = PredictionHistory::with_capacity(128, PredictionConfig::default());
        self.confirmed_predictions.clear();
        self.input_history = InputHistory::new();
        self.latest_server_tick = None;
        self.last_predicted_tick = None;
    }

    /// Validates and stages new game bounds before the owner builds a replacement world.
    pub fn prepare_travel(&mut self, game: &G, settings: &G::Settings) -> Result<(), AppError> {
        self.pending_bounds = Some(game.transform_bounds(settings)?);
        Ok(())
    }

    /// Simulation tick represented by the newest snapshot received from the server.
    #[must_use]
    pub const fn latest_server_tick(&self) -> Option<Tick> {
        self.latest_server_tick
    }

    /// Tick of the most recent command predicted by this client.
    #[must_use]
    pub const fn last_predicted_tick(&self) -> Option<Tick> {
        self.last_predicted_tick
    }

    /// Reconciled owner position for a confirmed command tick in the bounded history ring.
    #[must_use]
    pub fn confirmed_prediction_position(&self, tick: Tick) -> Option<Vec3> {
        self.confirmed_predictions
            .iter()
            .find(|(confirmed, _)| *confirmed == tick)
            .map(|(_, position)| *position)
    }

    /// Server tick currently selected for remote entity interpolation.
    #[must_use]
    pub fn interpolation_tick(&self) -> Option<Tick> {
        self.latest_server_tick
            .map(|tick| self.interpolation.render_tick(tick))
    }

    /// The client's transport, for an outer event loop to advance a deterministic transport.
    pub const fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Requests a lobby action.
    pub fn request(&mut self, request: LobbyRequest<G::Settings>) -> Result<(), AppError> {
        if !self.connected {
            return Err(NetError::NotConnected.into());
        }
        let payload = postcard::to_allocvec(&request).map_err(NetError::Decode)?;
        self.send(
            &Message::<G::Command>::Session(payload),
            engine_netcode::Channel::Reliable,
        )
    }

    /// Applies one client simulation tick: sends intent, predicts the local pawn, and updates
    /// snapshots/interpolation received from the server.
    pub fn tick(&mut self, world: &mut World, command: G::Command) -> Result<(), AppError> {
        self.last_predicted_tick = None;
        self.receive(world)?;
        if self.pending_travel.is_some() {
            return Ok(());
        }
        if let Some(pawn) = self.pawn {
            let command_tick = self.tick.advance(INPUT_LEAD_TICKS);
            let frame = InputFrame::new(command_tick, command.clone())?;
            self.input_history.push(frame)?;
            self.input_history
                .acknowledge_snapshots(self.snapshots.ack());
            if let Some(bundle) = self.input_history.bundle() {
                self.send(
                    &Message::Inputs(bundle),
                    engine_netcode::Channel::Unreliable,
                )?;
            }
            self.prediction
                .predict(world, pawn, command_tick, command)?;
            self.last_predicted_tick = Some(command_tick);
        }
        self.apply_interpolation(world)?;
        self.tick = self.tick.next();
        Ok(())
    }

    fn receive(&mut self, world: &mut World) -> Result<(), AppError> {
        while let Some(channel) = self.transport.recv(&mut self.recv_buffer)? {
            let message = decode::<G::Command>(&self.recv_buffer)?;
            if message.channel() != channel {
                return Err(
                    NetError::InvalidSnapshot("message arrived on the wrong channel").into(),
                );
            }
            match message {
                Message::Welcome { .. } => {
                    self.handshake
                        .on_server_message(Message::<G::Command>::Welcome {
                            version: engine_netcode::ProtocolVersion::CURRENT,
                            client_index: engine_netcode::ClientIndex::new(0),
                        })?;
                    self.connected = true;
                }
                Message::Reject { reason } => return Err(AppError::HandshakeRejected(reason)),
                Message::Session(bytes) => {
                    let reply: LobbyReply<G::Settings> =
                        postcard::from_bytes(&bytes).map_err(NetError::Decode)?;
                    self.apply_lobby_reply(reply);
                }
                Message::Possess { pawn } => self.pawn = Some(pawn),
                Message::Snapshot(packet) => self.apply_snapshot(world, &packet)?,
                Message::Hello { .. } | Message::Inputs(_) => {}
            }
        }
        Ok(())
    }

    fn apply_lobby_reply(&mut self, reply: LobbyReply<G::Settings>) {
        match reply {
            LobbyReply::Welcome { token, membership } => {
                self.resume_token = Some(token);
                self.membership = Some(membership);
            }
            LobbyReply::Approved(seat) => self.membership = Some(Membership::Seated(seat)),
            LobbyReply::Room(room) => {
                if room.phase == engine_sessions::Phase::InMatch
                    && self.room.as_ref().is_none_or(|current| {
                        current.phase != engine_sessions::Phase::InMatch
                            || current.settings != room.settings
                    })
                {
                    self.pending_travel = Some(room.settings.clone());
                }
                self.room = Some(room);
            }
            LobbyReply::Refused(error) => {
                platform::log_error(&format!("lobby request refused: {error}"));
            }
            LobbyReply::Removed => {
                self.membership = None;
                self.pawn = None;
            }
        }
    }

    fn apply_snapshot(
        &mut self,
        world: &mut World,
        packet: &SnapshotPacket,
    ) -> Result<(), AppError> {
        if packet.travel_epoch != self.travel_epoch {
            return Ok(());
        }
        self.snapshots.insert(packet);
        self.latest_server_tick = Some(packet.tick);
        for state in &packet.entities {
            if !self.replicated_entities.contains(&state.id)
                && self.replicated_entities.len() < CLIENT_ENTITY_CAPACITY
            {
                self.replicated_entities.push(state.id);
            }
            if world.is_alive(state.id) {
                apply_entity(world, state.id, state, &self.codecs, self.bounds)?;
                if Some(state.id) == self.pawn {
                    let predicted = self.prediction.position_at(packet.tick);
                    let authoritative = G::Predicted::capture(world, state.id)?;
                    let corrected =
                        self.prediction
                            .reconcile(world, state.id, packet.tick, &authoritative)?;
                    if let Some(predicted) = predicted {
                        let confirmed = if corrected {
                            G::Predicted::position(&authoritative)
                        } else {
                            predicted
                        };
                        if self.confirmed_predictions.len() == CONFIRMED_PREDICTION_HISTORY {
                            self.confirmed_predictions.pop_front();
                        }
                        self.confirmed_predictions
                            .push_back((packet.tick, confirmed));
                    }
                }
            }
        }
        Ok(())
    }

    fn apply_interpolation(&self, world: &mut World) -> Result<(), AppError> {
        let Some(server_tick) = self.latest_server_tick else {
            return Ok(());
        };
        let render_tick = self.interpolation.render_tick(server_tick);
        for id in &self.replicated_entities {
            if Some(*id) == self.pawn || !world.is_alive(*id) {
                continue;
            }
            let Some(timeline) = self.snapshots.timeline(*id) else {
                continue;
            };
            let Some(bracket) = engine_netcode::interpolate(
                timeline,
                render_tick,
                self.interpolation.max_extrapolation_ticks(),
            ) else {
                continue;
            };
            let Some(transform_component) = world.registry().id_of::<Transform>() else {
                continue;
            };
            let Some(older) = bracket
                .older
                .state
                .components
                .iter()
                .find(|value| value.component == transform_component.raw())
            else {
                continue;
            };
            let older_transform = decode_transform(&older.bytes, self.bounds)?;
            let transform = if bracket.older.tick == bracket.newer.tick {
                older_transform
            } else {
                let Some(newer) = bracket
                    .newer
                    .state
                    .components
                    .iter()
                    .find(|value| value.component == transform_component.raw())
                else {
                    continue;
                };
                engine_netcode::blend_transform(
                    &older_transform,
                    &decode_transform(&newer.bytes, self.bounds)?,
                    bracket.alpha,
                )?
            };
            world.insert(*id, transform)?;
        }
        Ok(())
    }

    fn send<I: engine_netcode::InputPayload>(
        &mut self,
        message: &Message<I>,
        channel: engine_netcode::Channel,
    ) -> Result<(), AppError> {
        encode(message, &mut self.send_buffer)?;
        self.transport.send(channel, &self.send_buffer)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::game::PlayerId;
    use crate::game::{Game, GameInfo};
    use engine_netcode::{
        ComponentState, EntityReplication, EntityState, GameId, GameProtocol, GameVersion,
        InputPayload, LinkConditions, Predicted, SnapshotPacket, encode_transform, loopback_pair,
    };
    use engine_sessions::PlayerName;
    use engine_world::Schedule;
    use glam::{Quat, Vec3};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    struct Command;

    impl InputPayload for Command {
        const MAX_BYTES: usize = 1;

        fn validate(&self) -> Result<(), NetError> {
            Ok(())
        }
    }

    struct TestPrediction;

    impl Predicted for TestPrediction {
        type State = Vec3;
        type Input = Command;

        fn capture(_world: &World, _id: NetId) -> Result<Self::State, NetError> {
            Ok(Vec3::ZERO)
        }

        fn restore(_world: &mut World, _id: NetId, _state: &Self::State) -> Result<(), NetError> {
            Ok(())
        }

        fn step(_world: &mut World, _id: NetId, _input: &Self::Input) -> Result<(), NetError> {
            Ok(())
        }

        fn position(state: &Self::State) -> Vec3 {
            *state
        }
    }

    struct TestGame;

    impl Game for TestGame {
        fn dynamic_entity_budget(&self) -> usize {
            0
        }

        type Command = Command;
        type Settings = ();

        fn info(&self) -> GameInfo {
            GameInfo {
                name: "snapshot-epoch-test",
                protocol_id: 1,
                protocol_version: 1,
            }
        }

        fn setup(
            &self,
            _world: &mut World,
            _schedule: &mut Schedule,
            _settings: &Self::Settings,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn join(
            &self,
            world: &mut World,
            _player: PlayerId,
            _name: &PlayerName,
        ) -> Result<NetId, AppError> {
            Ok(world.spawn()?)
        }

        fn leave(&self, _world: &mut World, _player: PlayerId) -> Result<(), AppError> {
            Ok(())
        }

        fn command(
            &self,
            _world: &mut World,
            _player: PlayerId,
            _command: &Self::Command,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    impl NetGame for TestGame {
        type Predicted = TestPrediction;

        fn replication(
            &self,
            _world: &World,
            _viewer: PlayerId,
            _entity: NetId,
        ) -> EntityReplication {
            EntityReplication::always(1.0)
        }

        fn transform_bounds(
            &self,
            _settings: &Self::Settings,
        ) -> Result<TransformBounds, AppError> {
            Ok(TransformBounds::new(
                glam::Vec3::splat(-32.0),
                glam::Vec3::splat(32.0),
                engine_netcode::DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
            )?)
        }

        fn protocol(&self) -> GameProtocol {
            GameProtocol {
                id: GameId::new(1),
                version: GameVersion::new(1),
            }
        }
    }

    #[test]
    fn snapshot_from_a_prior_world_epoch_is_dropped() {
        let (client_end, _server_end) =
            loopback_pair(LinkConditions::PERFECT, LinkConditions::PERFECT, 5);
        let game = TestGame;
        let mut client = NetClient::<TestGame, _>::connect(client_end, game.protocol(), &game, &())
            .expect("client builds");
        let mut world = World::new(3).expect("world");
        let id = world.spawn().expect("entity");
        world.insert(id, Transform::IDENTITY).expect("transform");
        client.set_travel_epoch(2);

        let target = Transform::new(Vec3::X, Quat::IDENTITY, Vec3::ONE).expect("target");
        let mut bytes = Vec::new();
        encode_transform(&target, client.bounds, &mut bytes).expect("encode transform");
        let component = world.registry().id_of::<Transform>().expect("component");
        let mut packet = SnapshotPacket::with_capacity(1, 0, 0);
        packet.set_travel_epoch(1);
        packet.entities.push(EntityState::new(
            id,
            vec![ComponentState {
                component: component.raw(),
                version: Tick::new(10),
                initial_only: false,
                removed: false,
                bytes,
            }],
        ));

        client
            .apply_snapshot(&mut world, &packet)
            .expect("drop old snapshot");
        assert_eq!(
            *world.get::<Transform>(id).expect("transform"),
            Transform::IDENTITY
        );
        assert_eq!(client.latest_server_tick, None);
    }
}
