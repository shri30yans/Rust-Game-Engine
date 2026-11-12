//! The active server tick allocates nothing after warm-up.
//!
//! The measured bound is exactly 0 allocations and 0 bytes per tick for two possessed peers, both
//! sending commands and receiving snapshots. Transport-owned queue buffers are outside this bound.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::alloc::System;
use std::num::NonZeroUsize;

use app::{AppConfig, AppError, Game, GameInfo, NetGame, NetServer, PlayerId, TickRate};
use engine_netcode::{
    Channel, EntityReplication, GameId, GameProtocol, GameVersion, InputFrame, InputHistory,
    InputPayload, LinkConditions, LoopbackHub, Message, NetError, Predicted, ProtocolVersion,
    ServerEvent, ServerTransport, encode,
};
use engine_sessions::{
    Admission, LobbyRequest, PlayerName, ResumeToken, RoomConfig, SessionError, TokenSource,
};
use engine_world::{NetId, Schedule, Transform, World};
use glam::Vec3;
use platform::{CpuProfile, DeviceClass, DeviceProfile};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::collections::VecDeque;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

struct IdleGame;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ServerMove(u8);

impl InputPayload for ServerMove {
    const MAX_BYTES: usize = 1;

    fn validate(&self) -> Result<(), NetError> {
        if self.0 <= 1 {
            Ok(())
        } else {
            Err(NetError::InvalidSnapshot("test movement outside range"))
        }
    }
}

impl Game for IdleGame {
    fn dynamic_entity_budget(&self) -> usize {
        0
    }

    type Command = ServerMove;
    type Settings = ();

    fn info(&self) -> GameInfo {
        GameInfo {
            name: "idle-server-test",
            protocol_id: 0x1d1e,
            protocol_version: 1,
        }
    }

    fn setup(
        &self,
        world: &mut World,
        _schedule: &mut Schedule,
        _settings: &Self::Settings,
    ) -> Result<(), AppError> {
        for _ in 0..2 {
            let pawn = world.spawn()?;
            world.insert(pawn, Transform::IDENTITY)?;
        }
        Ok(())
    }

    fn join(
        &self,
        world: &mut World,
        player: PlayerId,
        _name: &PlayerName,
    ) -> Result<NetId, AppError> {
        world
            .net_ids()
            .nth(
                usize::try_from(player.seat().raw())
                    .map_err(|_| engine_world::WorldError::Invariant("seat index overflow"))?,
            )
            .ok_or_else(|| engine_world::WorldError::Invariant("test pawn missing").into())
    }

    fn leave(&self, _world: &mut World, _player: PlayerId) -> Result<(), AppError> {
        Ok(())
    }

    fn command(
        &self,
        world: &mut World,
        player: PlayerId,
        command: &Self::Command,
    ) -> Result<(), AppError> {
        let id = world
            .net_ids()
            .nth(
                usize::try_from(player.seat().raw())
                    .map_err(|_| engine_world::WorldError::Invariant("seat index overflow"))?,
            )
            .ok_or(engine_world::WorldError::Invariant("test pawn missing"))?;
        let mut transform = world.get_mut::<Transform>(id)?;
        let translation = transform.translation() + Vec3::X * f32::from(command.0) * 0.01;
        transform.set_translation(translation)?;
        Ok(())
    }
}

struct IdlePrediction;

impl Predicted for IdlePrediction {
    type State = Vec3;
    type Input = ServerMove;

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

impl NetGame for IdleGame {
    type Predicted = IdlePrediction;

    fn replication(&self, _world: &World, _viewer: PlayerId, _entity: NetId) -> EntityReplication {
        EntityReplication::always(1.0)
    }

    fn transform_bounds(
        &self,
        _settings: &Self::Settings,
    ) -> Result<engine_netcode::TransformBounds, AppError> {
        Ok(engine_netcode::TransformBounds::new(
            Vec3::splat(-32.0),
            Vec3::splat(32.0),
            engine_netcode::DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
        )?)
    }

    fn protocol(&self) -> GameProtocol {
        GameProtocol {
            id: GameId::new(0x1d1e),
            version: GameVersion::new(1),
        }
    }
}

#[derive(Default)]
struct Tokens(u8);

impl TokenSource for Tokens {
    fn next_token(&mut self) -> Result<ResumeToken, SessionError> {
        self.0 = self.0.wrapping_add(1);
        Ok(ResumeToken::from_bytes([self.0; 32]))
    }
}

struct QueuedEvent {
    event: ServerEvent,
    bytes: Vec<u8>,
}

struct SinkTransport {
    events: VecDeque<QueuedEvent>,
    inputs: Vec<(engine_netcode::ClientId, InputHistory<ServerMove>)>,
    sent_to: Vec<engine_netcode::ClientId>,
}

impl SinkTransport {
    fn new(ids: [engine_netcode::ClientId; 2]) -> Result<Self, NetError> {
        let [first_id, second_id] = ids;
        let mut events = VecDeque::with_capacity(16);
        for id in ids {
            events.push_back(QueuedEvent {
                event: ServerEvent::Connected(id),
                bytes: Vec::new(),
            });
        }
        let hello = Message::<()>::Hello {
            version: ProtocolVersion::CURRENT,
            game: GameProtocol {
                id: GameId::new(0x1d1e),
                version: GameVersion::new(1),
            },
            client_nonce: 1,
        };
        for id in ids {
            Self::push_message(&mut events, id, Channel::Reliable, &hello)?;
        }
        for (id, name) in [(first_id, "peer-one"), (second_id, "peer-two")] {
            let request = LobbyRequest::<()>::Hello {
                name: PlayerName::new(name).map_err(|_| NetError::InvalidSnapshot("test name"))?,
                resume: None,
            };
            Self::push_request(&mut events, id, request)?;
        }
        for id in ids {
            Self::push_request(&mut events, id, LobbyRequest::SetReady(true))?;
        }
        Self::push_request(&mut events, first_id, LobbyRequest::Start)?;
        Ok(Self {
            events,
            inputs: ids
                .into_iter()
                .map(|id| (id, InputHistory::new()))
                .collect(),
            sent_to: Vec::with_capacity(2),
        })
    }

    fn push_request(
        events: &mut VecDeque<QueuedEvent>,
        id: engine_netcode::ClientId,
        request: LobbyRequest<()>,
    ) -> Result<(), NetError> {
        let payload = postcard::to_allocvec(&request).map_err(NetError::Decode)?;
        Self::push_message(
            events,
            id,
            Channel::Reliable,
            &Message::<()>::Session(payload),
        )
    }

    fn queue_command(&mut self, id: engine_netcode::ClientId, tick: engine_core::Tick) {
        let Some((_, history)) = self.inputs.iter_mut().find(|(client, _)| *client == id) else {
            return;
        };
        let Ok(frame) = InputFrame::new(tick, ServerMove(1)) else {
            return;
        };
        if history.push(frame).is_err() {
            return;
        }
        let Some(bundle) = history.bundle() else {
            return;
        };
        let mut bytes = Vec::with_capacity(engine_netcode::MAX_MESSAGE_BYTES);
        if encode(&Message::Inputs(bundle), &mut bytes).is_ok() {
            self.events.push_back(QueuedEvent {
                event: ServerEvent::Message(id, Channel::Unreliable),
                bytes,
            });
        }
    }

    fn push_message<I: InputPayload>(
        events: &mut VecDeque<QueuedEvent>,
        id: engine_netcode::ClientId,
        channel: Channel,
        message: &Message<I>,
    ) -> Result<(), NetError> {
        let mut bytes = Vec::with_capacity(engine_netcode::MAX_MESSAGE_BYTES);
        encode(message, &mut bytes)?;
        events.push_back(QueuedEvent {
            event: ServerEvent::Message(id, channel),
            bytes,
        });
        Ok(())
    }
}

impl ServerTransport for SinkTransport {
    fn recv(&mut self, out: &mut Vec<u8>) -> Option<ServerEvent> {
        let queued = self.events.pop_front()?;
        out.clear();
        out.extend_from_slice(&queued.bytes);
        Some(queued.event)
    }

    fn send(
        &mut self,
        client: engine_netcode::ClientId,
        _channel: Channel,
        _bytes: &[u8],
    ) -> Result<(), NetError> {
        self.sent_to.push(client);
        Ok(())
    }

    fn disconnect(&mut self, _client: engine_netcode::ClientId) -> Result<(), NetError> {
        Ok(())
    }
}

fn profile() -> DeviceProfile {
    DeviceProfile {
        // The inline background pool keeps unrelated worker allocations out of the counter.
        class: DeviceClass::Web,
        touch: false,
        cpu: CpuProfile {
            logical_cores: NonZeroUsize::new(2).expect("non-zero"),
            performance_cores: NonZeroUsize::new(2).expect("non-zero"),
        },
        memory_bytes: None,
        gpu: None,
    }
}

#[test]
fn active_server_ticks_allocate_nothing_after_warm_up() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig {
        tick_rate: TickRate::new(60).expect("tick rate"),
        seed: 4,
        max_catch_up_ticks: 8,
        job_workers: Some(0),
    };
    let mut hub = LoopbackHub::new(2, 9);
    let _first_end = hub
        .connect(LinkConditions::PERFECT, LinkConditions::PERFECT)
        .expect("first test link");
    let _second_end = hub
        .connect(LinkConditions::PERFECT, LinkConditions::PERFECT)
        .expect("second test link");
    let mut first_event_buffer = Vec::new();
    let first_id = match hub
        .recv(&mut first_event_buffer)
        .ok_or("first connected event missing")?
    {
        ServerEvent::Connected(id) => id,
        event => return Err(format!("unexpected first event: {event:?}").into()),
    };
    let second_id = match hub
        .recv(&mut first_event_buffer)
        .ok_or("second connected event missing")?
    {
        ServerEvent::Connected(id) => id,
        event => return Err(format!("unexpected second event: {event:?}").into()),
    };
    let mut server = NetServer::new(
        IdleGame,
        &config,
        &profile(),
        RoomConfig::new(2, 2, 2, Admission::Open, 0).expect("room config"),
        (),
        SinkTransport::new([first_id, second_id]).expect("scripted transport"),
        Tokens::default(),
    )
    .expect("server");
    for _ in 0..32 {
        server.tick().expect("warm-up tick");
    }
    assert_eq!(server.lobby().seats.len(), 2);
    server.transport_mut().sent_to.clear();

    for raw_tick in 32..1056 {
        server
            .transport_mut()
            .queue_command(first_id, engine_core::Tick::new(raw_tick));
        server
            .transport_mut()
            .queue_command(second_id, engine_core::Tick::new(raw_tick));
        let region = Region::new(GLOBAL);
        server.tick().expect("measured tick");
        let stats = region.change();
        assert_eq!(stats.allocations, 0, "server tick allocated: {stats:?}");
        assert_eq!(stats.reallocations, 0, "server tick reallocated: {stats:?}");
        assert_eq!(
            stats.bytes_allocated, 0,
            "server tick allocated bytes: {stats:?}"
        );
        let sends = &server.transport_mut().sent_to;
        assert_eq!(
            sends.len(),
            2,
            "each possessed peer must receive a snapshot"
        );
        assert_eq!(
            sends.iter().filter(|client| **client == first_id).count(),
            1,
            "first possessed peer must receive one snapshot"
        );
        assert_eq!(
            sends.iter().filter(|client| **client == second_id).count(),
            1,
            "second possessed peer must receive one snapshot"
        );
        server.transport_mut().sent_to.clear();
    }
    let mut pawns = server.world().net_ids();
    let first_pawn = pawns.next().ok_or("first pawn missing")?;
    let second_pawn = pawns.next().ok_or("second pawn missing")?;
    assert!(
        server.world().get::<Transform>(first_pawn)?.translation().x > 0.0,
        "first peer's commands should move its pawn"
    );
    assert!(
        server
            .world()
            .get::<Transform>(second_pawn)?
            .translation()
            .x
            > 0.0,
        "second peer's commands should move its pawn"
    );
    Ok(())
}
