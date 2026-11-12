//! Headless room, travel, prediction and resume over a lossy multi-client transport.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{collections::VecDeque, num::NonZeroUsize};

use app::{
    App, AppConfig, AppError, Game, GameInfo, NetClient, NetGame, NetServer, PlayerId, TickRate,
};
use engine_core::{StateHasher, Tick};
use engine_netcode::{
    EntityReplication, GameId, GameProtocol, GameVersion, InputPayload, LinkConditions,
    LoopbackEnd, LoopbackHub, NetError, Predicted, RejectReason,
};
use engine_sessions::{
    Admission, LobbyRequest, Membership, PlayerName, ResumeToken, RoomConfig, SessionError,
    TokenSource,
};
use engine_world::{NetId, Resource, Schedule, SystemContext, Transform, World};
use glam::Vec3;
use platform::{CpuProfile, DeviceClass, DeviceProfile};
use serde::{Deserialize, Serialize};

const LINK: LinkConditions = LinkConditions {
    latency_ticks: 4,
    jitter_ticks: 1,
    loss_per_mille: 50,
    reorder_per_mille: 0,
};
const SERVER_HISTORY_TICKS: usize = 128;
const PREDICTION_SAMPLE_INTERVAL: u32 = 10;
const INTERPOLATION_SAMPLE_INTERVAL: u32 = 5;
// The 256 m arena selects 18 bits: 2^18 - 1 levels, so its wire step is 256 / 262143 m.
const TEST_TRANSFORM_BOUNDS_METERS: f32 = 128.0;
const TEST_POSITION_PRECISION_METERS: f32 = engine_netcode::DEFAULT_CHARACTER_MOVEMENT_PRECISION_M;
const TEST_POSITION_WIRE_STEP_METERS: f32 = 256.0 / 262_143.0;
// Prediction allows 1 cm plus one actual wire position step.
const PREDICTION_TOLERANCE_METERS: f32 = 0.01 + TEST_POSITION_WIRE_STEP_METERS;
const INTERPOLATION_TOLERANCE_METERS: f32 = 0.5;
const SERVER_PUSH_TICK: u32 = 100;
/// Maximum predicted ticks after the correcting snapshot before a confirmed sample must converge.
const MAX_RECONCILIATION_TICKS: u32 = 16;

#[derive(Clone, Copy)]
struct ServerPositions {
    tick: Tick,
    first: Vec3,
    second: Vec3,
}

#[derive(Clone, Copy)]
struct PredictionSample {
    tick: Tick,
    after_correction: bool,
}

#[derive(Default)]
struct CorrectionProgress {
    snapshot_tick: Option<Tick>,
    corrected_samples: usize,
    first_converged_after_ticks: Option<u32>,
}

struct ServerPush {
    target: NetId,
    applied: bool,
}

impl Resource for ServerPush {
    fn hash_into(&self, hasher: &mut StateHasher) {
        hasher.write_u32(self.target.index());
        hasher.write_u32(self.target.generation());
        hasher.write_u8(u8::from(self.applied));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Move(i8);

impl InputPayload for Move {
    const MAX_BYTES: usize = 1;

    fn validate(&self) -> Result<(), NetError> {
        if (-1..=1).contains(&self.0) {
            Ok(())
        } else {
            Err(NetError::InvalidSnapshot("test movement outside range"))
        }
    }
}

struct PointPrediction;

impl Predicted for PointPrediction {
    type State = Vec3;
    type Input = Move;

    fn capture(world: &World, id: NetId) -> Result<Self::State, NetError> {
        world
            .get::<Transform>(id)
            .map(|transform| transform.translation())
            .map_err(|error| NetError::World(error.to_string()))
    }

    fn restore(world: &mut World, id: NetId, state: &Self::State) -> Result<(), NetError> {
        world
            .get_mut::<Transform>(id)
            .and_then(|mut transform| transform.set_translation(*state))
            .map_err(|error| NetError::World(error.to_string()))?;
        Ok(())
    }

    fn step(world: &mut World, id: NetId, input: &Self::Input) -> Result<(), NetError> {
        move_point(world, id, *input).map_err(|error| NetError::World(error.to_string()))
    }

    fn position(state: &Self::State) -> Vec3 {
        *state
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Settings {
    arena: u8,
}

struct PointGame;

impl Game for PointGame {
    fn dynamic_entity_budget(&self) -> usize {
        0
    }

    type Command = Move;
    type Settings = Settings;

    fn info(&self) -> GameInfo {
        GameInfo {
            name: "point-net-test",
            protocol_id: 0x9e71,
            protocol_version: 1,
        }
    }

    fn setup(
        &self,
        world: &mut World,
        schedule: &mut Schedule,
        _settings: &Self::Settings,
    ) -> Result<(), AppError> {
        schedule.add_system("test_server_push", server_push)?;
        for _ in 0..2 {
            let id = world.spawn()?;
            world.insert(id, Transform::IDENTITY)?;
        }
        Ok(())
    }

    fn join(
        &self,
        world: &mut World,
        player: PlayerId,
        _name: &PlayerName,
    ) -> Result<NetId, AppError> {
        let index = usize::try_from(player.seat().raw())
            .map_err(|_| engine_world::WorldError::Invariant("seat index overflow"))?;
        let pawn = world
            .net_ids()
            .nth(index)
            .ok_or(engine_world::WorldError::Invariant("test pawn missing"))?;
        if world.resource::<ServerPush>().is_err() {
            world.insert_resource(ServerPush {
                target: pawn,
                applied: false,
            })?;
        }
        Ok(pawn)
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
        Ok(move_point(world, id, *command)?)
    }
}

impl NetGame for PointGame {
    type Predicted = PointPrediction;

    fn replication(&self, _world: &World, _viewer: PlayerId, _entity: NetId) -> EntityReplication {
        EntityReplication::always(1.0)
    }

    fn transform_bounds(
        &self,
        _settings: &Self::Settings,
    ) -> Result<engine_netcode::TransformBounds, AppError> {
        Ok(engine_netcode::TransformBounds::new(
            Vec3::splat(-TEST_TRANSFORM_BOUNDS_METERS),
            Vec3::splat(TEST_TRANSFORM_BOUNDS_METERS),
            TEST_POSITION_PRECISION_METERS,
        )?)
    }

    fn protocol(&self) -> GameProtocol {
        GameProtocol {
            id: GameId::new(0x9e71),
            version: GameVersion::new(1),
        }
    }
}

fn server_push(
    world: &mut World,
    context: &mut SystemContext<'_>,
) -> Result<(), engine_world::WorldError> {
    let Ok(push) = world.resource::<ServerPush>() else {
        return Ok(());
    };
    if push.applied || context.tick.raw() != SERVER_PUSH_TICK {
        return Ok(());
    }
    let target = push.target;
    let position = world.get::<Transform>(target)?.translation() + Vec3::X;
    world
        .get_mut::<Transform>(target)?
        .set_translation(position)?;
    world.resource_mut::<ServerPush>()?.applied = true;
    Ok(())
}

fn move_point(world: &mut World, id: NetId, command: Move) -> Result<(), engine_world::WorldError> {
    let mut transform = world.get_mut::<Transform>(id)?;
    let next_position = transform.translation() + Vec3::X * f32::from(command.0) * 0.2;
    transform.set_translation(next_position)
}

fn scripted_move(raw_tick: u32, period: u32, offset: u32) -> Move {
    let phase = (raw_tick + offset) % (period * 2);
    if phase < period { Move(1) } else { Move(-1) }
}

#[test]
fn disabling_prediction_fails_the_confirmed_position_check() {
    let authoritative = Vec3::X;
    let client_without_prediction = Vec3::ZERO;
    assert!(!prediction_matches(
        client_without_prediction,
        authoritative
    ));
}

#[derive(Default)]
struct CounterTokens(u8);

impl TokenSource for CounterTokens {
    fn next_token(&mut self) -> Result<ResumeToken, SessionError> {
        self.0 = self.0.wrapping_add(1);
        Ok(ResumeToken::from_bytes([self.0; 32]))
    }
}

fn profile() -> DeviceProfile {
    DeviceProfile {
        class: DeviceClass::Server,
        touch: false,
        cpu: CpuProfile {
            logical_cores: NonZeroUsize::new(2).expect("non-zero"),
            performance_cores: NonZeroUsize::new(2).expect("non-zero"),
        },
        memory_bytes: None,
        gpu: None,
    }
}

fn config() -> AppConfig {
    AppConfig {
        tick_rate: TickRate::new(60).expect("tick rate"),
        seed: 17,
        max_catch_up_ticks: 8,
        job_workers: Some(0),
    }
}

type Client = NetClient<PointGame, LoopbackEnd>;
type Server = NetServer<PointGame, LoopbackHub, CounterTokens>;

fn advance_client(client: &mut Client, app: &mut App<PointGame>, tick: Tick, command: Move) {
    client.transport_mut().advance_to(tick);
    client.tick(app.world_mut(), command).expect("client tick");
    if let Some(settings) = client.take_pending_travel() {
        client
            .prepare_travel(&PointGame, &settings)
            .expect("client bounds validate");
        app.travel(&settings).expect("client travels");
        client.set_travel_epoch(app.travel_epoch());
    }
}

fn advance_tick(
    server: &mut Server,
    first: &mut Client,
    first_app: &mut App<PointGame>,
    second: &mut Client,
    second_app: &mut App<PointGame>,
    raw_tick: u32,
) {
    advance_tick_commands(
        server,
        (first, first_app),
        (second, second_app),
        raw_tick,
        (Move(0), Move(0)),
    );
}

fn advance_tick_commands(
    server: &mut Server,
    first: (&mut Client, &mut App<PointGame>),
    second: (&mut Client, &mut App<PointGame>),
    raw_tick: u32,
    commands: (Move, Move),
) -> Tick {
    let tick = Tick::new(raw_tick);
    server.transport_mut().advance_to(tick);
    let server_tick = server.tick().expect("server tick").tick;
    advance_client(first.0, first.1, tick, commands.0);
    advance_client(second.0, second.1, tick, commands.1);
    server_tick
}

fn append_server_positions(
    history: &mut VecDeque<ServerPositions>,
    tick: Tick,
    server: &Server,
    first_pawn: NetId,
    second_pawn: NetId,
) {
    if history.len() == SERVER_HISTORY_TICKS {
        history.pop_front();
    }
    history.push_back(ServerPositions {
        tick,
        first: server
            .world()
            .get::<Transform>(first_pawn)
            .expect("first pawn")
            .translation(),
        second: server
            .world()
            .get::<Transform>(second_pawn)
            .expect("second pawn")
            .translation(),
    });
}

fn append_prediction_sample(
    samples: &mut VecDeque<PredictionSample>,
    client: &Client,
    after_correction: bool,
) {
    let Some(tick) = client.last_predicted_tick() else {
        return;
    };
    if samples.len() == SERVER_HISTORY_TICKS {
        samples.pop_front();
    }
    samples.push_back(PredictionSample {
        tick,
        after_correction,
    });
}

fn check_confirmed_predictions(
    samples: &mut VecDeque<PredictionSample>,
    server_history: &VecDeque<ServerPositions>,
    client: &Client,
    authoritative_position: impl Fn(&ServerPositions) -> Vec3,
    checked: &mut usize,
    correction: &mut CorrectionProgress,
) {
    // A prediction sample is checked only after a received snapshot tick reaches its command tick.
    let Some(confirmed_tick) = client.latest_server_tick() else {
        return;
    };
    while samples
        .front()
        .is_some_and(|sample| sample.tick == confirmed_tick || confirmed_tick.is_after(sample.tick))
    {
        let Some(sample) = samples.pop_front() else {
            break;
        };
        let correction_tick = Tick::new(SERVER_PUSH_TICK);
        if (sample.tick == correction_tick || sample.tick.is_after(correction_tick))
            && !sample.after_correction
        {
            continue;
        }
        if sample.tick.raw() % PREDICTION_SAMPLE_INTERVAL != 0 {
            continue;
        }
        let Some(authoritative) = server_history
            .iter()
            .find(|positions| positions.tick == sample.tick)
            .map(&authoritative_position)
        else {
            continue;
        };
        let Some(reconciled) = client.confirmed_prediction_position(sample.tick) else {
            continue;
        };
        let matches = prediction_matches(reconciled, authoritative);
        let error = reconciled.distance(authoritative);
        assert!(
            matches,
            "prediction at confirmed command tick {} (after_correction={}) differs by {error} m: client={:?}, server={authoritative:?}",
            sample.tick.raw(),
            sample.after_correction,
            reconciled
        );
        *checked += 1;
        if sample.after_correction
            && let Some(snapshot_tick) = correction.snapshot_tick
        {
            if correction.corrected_samples == 0 {
                let delay = sample.tick.distance(snapshot_tick);
                assert!(
                    delay <= MAX_RECONCILIATION_TICKS,
                    "prediction did not converge within {MAX_RECONCILIATION_TICKS} ticks after correcting snapshot {snapshot_tick:?}; first checked tick was {:?} ({delay} ticks later)",
                    sample.tick
                );
                correction.first_converged_after_ticks = Some(delay);
            }
            correction.corrected_samples += 1;
        }
    }
}

fn prediction_matches(predicted: Vec3, authoritative: Vec3) -> bool {
    predicted.distance(authoritative) <= PREDICTION_TOLERANCE_METERS
}

fn check_remote_interpolation(
    client: &Client,
    world: &World,
    remote_pawn: NetId,
    server_history: &VecDeque<ServerPositions>,
    authoritative_position: impl Fn(&ServerPositions) -> Vec3,
) -> bool {
    let Some(render_tick) = client.interpolation_tick() else {
        return false;
    };
    let Some(authoritative) = server_history
        .iter()
        .find(|positions| positions.tick == render_tick)
        .map(authoritative_position)
    else {
        return false;
    };
    let rendered = world
        .get::<Transform>(remote_pawn)
        .expect("remote pawn")
        .translation();
    let error = rendered.distance(authoritative);
    assert!(
        error <= INTERPOLATION_TOLERANCE_METERS,
        "remote interpolation at render tick {} differs by {error} m: client={rendered:?}, server={authoritative:?}",
        render_tick.raw()
    );
    true
}

#[test]
fn two_clients_travel_resume_and_return_to_a_second_match() {
    let bounds = PointGame
        .transform_bounds(&Settings { arena: 0 })
        .expect("test arena bounds");
    assert!(bounds.position_step_m() <= f64::from(TEST_POSITION_PRECISION_METERS));
    assert!((bounds.position_step_m() - f64::from(TEST_POSITION_WIRE_STEP_METERS)).abs() < 1e-7);
    let mut hub = LoopbackHub::new(3, 42);
    let first_end = hub.connect(LINK, LINK).expect("first link");
    let second_end = hub.connect(LINK, LINK).expect("second link");
    let mut server = Server::new(
        PointGame,
        &config(),
        &profile(),
        RoomConfig::new(2, 1, 2, Admission::Open, 2).expect("room config"),
        Settings { arena: 0 },
        hub,
        CounterTokens::default(),
    )
    .expect("server");
    let first_game = PointGame;
    let second_game = PointGame;
    let settings = Settings { arena: 0 };
    let mut first = Client::connect(first_end, first_game.protocol(), &first_game, &settings)
        .expect("client one");
    let mut second = Client::connect(second_end, second_game.protocol(), &second_game, &settings)
        .expect("client two");
    let mut first_app = App::new(PointGame, &Settings { arena: 0 }, &config(), &profile())
        .expect("first client app");
    let mut second_app = App::new(PointGame, &Settings { arena: 0 }, &config(), &profile())
        .expect("second client app");
    let first_name = PlayerName::new("one").expect("valid name");
    let second_name = PlayerName::new("two").expect("valid name");
    let mut first_hello = false;
    let mut second_hello = false;
    let mut first_ready = false;
    let mut second_ready = false;
    let mut started = false;

    for tick in 0..80 {
        advance_tick(
            &mut server,
            &mut first,
            &mut first_app,
            &mut second,
            &mut second_app,
            tick,
        );
        if !first_hello
            && first
                .request(LobbyRequest::Hello {
                    name: first_name.clone(),
                    resume: None,
                })
                .is_ok()
        {
            first_hello = true;
        }
        if !second_hello
            && second
                .request(LobbyRequest::Hello {
                    name: second_name.clone(),
                    resume: None,
                })
                .is_ok()
        {
            second_hello = true;
        }
        if !first_ready && first.membership().is_some() {
            first
                .request(LobbyRequest::SetReady(true))
                .expect("ready one");
            first_ready = true;
        }
        if !second_ready && second.membership().is_some() {
            second
                .request(LobbyRequest::SetReady(true))
                .expect("ready two");
            second_ready = true;
        }
        if !started
            && first.lobby().is_some_and(|room| {
                room.seats.len() == 2 && room.seats.iter().all(|seat| seat.ready)
            })
        {
            first.request(LobbyRequest::Start).expect("host starts");
            started = true;
        }
        if server.lobby().phase == engine_sessions::Phase::InMatch
            && first.pawn().is_some()
            && second.pawn().is_some()
            && first_app.travel_epoch() == 1
            && second_app.travel_epoch() == 1
        {
            break;
        }
    }
    assert!(started);
    let first_seat = first.membership().expect("first seat");
    let second_seat = second.membership().expect("second seat");
    assert!(matches!(first_seat, Membership::Seated(_)));
    assert!(matches!(second_seat, Membership::Seated(_)));
    let first_pawn = first.pawn().expect("first possession");
    let second_pawn = second.pawn().expect("second possession");
    assert_eq!(first_app.travel_epoch(), 1);
    assert_eq!(second_app.travel_epoch(), 1);

    for tick in 80..100 {
        advance_tick_commands(
            &mut server,
            (&mut first, &mut first_app),
            (&mut second, &mut second_app),
            tick,
            (Move(0), Move(0)),
        );
    }
    let mut server_history = VecDeque::with_capacity(SERVER_HISTORY_TICKS);
    let mut first_predictions = VecDeque::with_capacity(SERVER_HISTORY_TICKS);
    let mut second_predictions = VecDeque::with_capacity(SERVER_HISTORY_TICKS);
    let mut first_prediction_checks = 0;
    let mut second_prediction_checks = 0;
    let mut first_correction = CorrectionProgress::default();
    let mut second_correction = CorrectionProgress::default();
    let mut server_push_observed = false;
    let mut first_interpolation_checks = 0;
    let mut second_interpolation_checks = 0;
    for tick in 100..680 {
        let first_input_tick = first
            .last_predicted_tick()
            .map_or(Tick::new(tick).advance(6), Tick::next)
            .raw();
        let second_input_tick = second
            .last_predicted_tick()
            .map_or(Tick::new(tick).advance(6), Tick::next)
            .raw();
        let commands = (
            scripted_move(first_input_tick, 29, 0),
            scripted_move(second_input_tick, 23, 11),
        );
        let server_tick = advance_tick_commands(
            &mut server,
            (&mut first, &mut first_app),
            (&mut second, &mut second_app),
            tick,
            commands,
        );
        if first_correction.snapshot_tick.is_none()
            && let Some(snapshot_tick) = first.latest_server_tick().filter(|snapshot_tick| {
                *snapshot_tick == Tick::new(SERVER_PUSH_TICK)
                    || snapshot_tick.is_after(Tick::new(SERVER_PUSH_TICK))
            })
        {
            first_correction.snapshot_tick = Some(snapshot_tick);
        }
        if second_correction.snapshot_tick.is_none()
            && let Some(snapshot_tick) = second.latest_server_tick().filter(|snapshot_tick| {
                *snapshot_tick == Tick::new(SERVER_PUSH_TICK)
                    || snapshot_tick.is_after(Tick::new(SERVER_PUSH_TICK))
            })
        {
            second_correction.snapshot_tick = Some(snapshot_tick);
        }
        append_server_positions(
            &mut server_history,
            server_tick,
            &server,
            first_pawn,
            second_pawn,
        );
        if server_tick == Tick::new(SERVER_PUSH_TICK) {
            let current = server_history.back().expect("correction position");
            let previous = server_history
                .iter()
                .rev()
                .nth(1)
                .expect("pre-correction position");
            let displacement = current.first.distance(previous.first);
            assert!(
                displacement >= 0.75,
                "the server push should add at least 0.75 m in its tick, got {displacement} m"
            );
            server_push_observed = true;
        }
        append_prediction_sample(
            &mut first_predictions,
            &first,
            first_correction.snapshot_tick.is_some(),
        );
        append_prediction_sample(
            &mut second_predictions,
            &second,
            second_correction.snapshot_tick.is_some(),
        );
        check_confirmed_predictions(
            &mut first_predictions,
            &server_history,
            &first,
            |positions| positions.first,
            &mut first_prediction_checks,
            &mut first_correction,
        );
        check_confirmed_predictions(
            &mut second_predictions,
            &server_history,
            &second,
            |positions| positions.second,
            &mut second_prediction_checks,
            &mut second_correction,
        );
        if tick % INTERPOLATION_SAMPLE_INTERVAL == 0 {
            first_interpolation_checks += usize::from(check_remote_interpolation(
                &first,
                first_app.world(),
                second_pawn,
                &server_history,
                |positions| positions.second,
            ));
            second_interpolation_checks += usize::from(check_remote_interpolation(
                &second,
                second_app.world(),
                first_pawn,
                &server_history,
                |positions| positions.first,
            ));
        }
    }
    assert!(
        first_prediction_checks >= 20 && second_prediction_checks >= 20,
        "expected many confirmed moving prediction samples; first={first_prediction_checks}, second={second_prediction_checks}"
    );
    assert!(
        server_push_observed,
        "the server correction system did not run"
    );
    assert!(first_correction.snapshot_tick.is_some());
    assert!(second_correction.snapshot_tick.is_some());
    assert!(
        first_correction.corrected_samples >= 20
            && second_correction.corrected_samples >= 20
            && first_correction.first_converged_after_ticks.is_some(),
        "expected convergence and continued confirmed prediction after correction; first={}, second={}, delay={:?}",
        first_correction.corrected_samples,
        second_correction.corrected_samples,
        first_correction.first_converged_after_ticks
    );
    assert!(
        first_interpolation_checks >= 20 && second_interpolation_checks >= 20,
        "expected many moving interpolation samples; first={first_interpolation_checks}, second={second_interpolation_checks}"
    );

    let token = second.resume_token().expect("resume token");
    let old_pawn = second.pawn().expect("second pawn");
    drop(second);
    drop(second_app);
    let (next_end, mut replacement_app) = {
        let hub = server.transport_mut();
        let end = hub.connect(LINK, LINK).expect("resume link");
        let app = App::new(PointGame, &Settings { arena: 0 }, &config(), &profile())
            .expect("replacement app");
        (end, app)
    };
    let resumed_game = PointGame;
    let mut resumed = Client::connect(
        next_end,
        resumed_game.protocol(),
        &resumed_game,
        &Settings { arena: 0 },
    )
    .expect("resumed client");
    let mut resume_requested = false;
    for tick in 680..740 {
        server.transport_mut().advance_to(Tick::new(tick));
        resumed.transport_mut().advance_to(Tick::new(tick));
        server.tick().expect("resume server tick");
        resumed
            .tick(replacement_app.world_mut(), Move(0))
            .expect("resume client tick");
        if let Some(settings) = resumed.take_pending_travel() {
            resumed
                .prepare_travel(&resumed_game, &settings)
                .expect("resume bounds validate");
            replacement_app.travel(&settings).expect("resume travel");
            resumed.set_travel_epoch(replacement_app.travel_epoch());
        }
        if !resume_requested
            && resumed
                .request(LobbyRequest::Hello {
                    name: second_name.clone(),
                    resume: Some(token),
                })
                .is_ok()
        {
            resume_requested = true;
        }
        if resumed.pawn().is_some() && resumed.membership() == Some(second_seat) {
            break;
        }
    }
    assert!(resume_requested);
    assert_eq!(resumed.membership(), Some(second_seat));
    assert_eq!(resumed.pawn(), Some(old_pawn));

    server.end_match().expect("end match");
    server.return_to_lobby().expect("return to lobby");
    let mut first_readied_again = false;
    let mut second_readied_again = false;
    let mut started_again = false;
    for tick in 740..850 {
        advance_tick(
            &mut server,
            &mut first,
            &mut first_app,
            &mut resumed,
            &mut replacement_app,
            tick,
        );
        if !first_readied_again
            && first
                .lobby()
                .is_some_and(|room| room.phase == engine_sessions::Phase::Lobby)
        {
            first
                .request(LobbyRequest::SetReady(true))
                .expect("ready for second match");
            first_readied_again = true;
        }
        if !second_readied_again
            && resumed
                .lobby()
                .is_some_and(|room| room.phase == engine_sessions::Phase::Lobby)
        {
            resumed
                .request(LobbyRequest::SetReady(true))
                .expect("ready for second match");
            second_readied_again = true;
        }
        if !started_again
            && first.lobby().is_some_and(|room| {
                room.phase == engine_sessions::Phase::Lobby
                    && room.seats.len() == 2
                    && room.seats.iter().all(|seat| seat.ready)
            })
        {
            first
                .request(LobbyRequest::Start)
                .expect("start second match");
            started_again = true;
        }
        if server.lobby().phase == engine_sessions::Phase::InMatch
            && first_app.travel_epoch() == 2
            && replacement_app.travel_epoch() == 2
            && first.pawn().is_some()
            && resumed.pawn().is_some()
        {
            break;
        }
    }
    assert!(started_again);
    assert_eq!(first_app.travel_epoch(), 2);
    assert_eq!(replacement_app.travel_epoch(), 2);
    assert_eq!(first.membership(), Some(first_seat));
    assert_eq!(resumed.membership(), Some(second_seat));
}

#[test]
fn a_client_with_a_different_game_protocol_is_refused() {
    let mut hub = LoopbackHub::new(1, 7);
    let end = hub.connect(LINK, LINK).expect("link");
    let mut server = Server::new(
        PointGame,
        &config(),
        &profile(),
        RoomConfig::new(1, 1, 1, Admission::Open, 0).expect("room config"),
        Settings { arena: 0 },
        hub,
        CounterTokens::default(),
    )
    .expect("server");
    let game = PointGame;
    let settings = Settings { arena: 0 };
    let mut client = Client::connect(
        end,
        GameProtocol {
            id: GameId::new(0x1111),
            version: GameVersion::new(1),
        },
        &game,
        &settings,
    )
    .expect("wrong protocol client");
    let mut app =
        App::new(PointGame, &Settings { arena: 0 }, &config(), &profile()).expect("client app");
    let mut rejected = false;
    for raw_tick in 0..16 {
        let tick = Tick::new(raw_tick);
        server.transport_mut().advance_to(tick);
        client.transport_mut().advance_to(tick);
        server.tick().expect("server tick");
        if matches!(
            client.tick(app.world_mut(), Move(0)),
            Err(AppError::HandshakeRejected(RejectReason::WrongGame))
        ) {
            rejected = true;
            break;
        }
    }
    assert!(
        rejected,
        "wrong protocol should be refused by the handshake"
    );
}
