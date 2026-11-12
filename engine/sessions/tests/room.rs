//! Room behaviour: scenarios for each rule, and a property test that drives random command
//! sequences and checks every room invariant after each one.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use engine_core::Tick;
use engine_sessions::{
    Admission, JoinOutcome, Occupant, Phase, PlayerName, ResumeToken, Room, RoomConfig, RoomEvent,
    SeatId, SessionError, TeamId,
};
use proptest::prelude::*;

fn name(text: &str) -> PlayerName {
    PlayerName::new(text).unwrap()
}

fn token(n: u8) -> ResumeToken {
    ResumeToken::from_bytes([n; 32])
}

fn room(seats: u8, teams: u8, min: u8, admission: Admission) -> Room<u32> {
    Room::new(
        RoomConfig::new(seats, teams, min, admission, 10).unwrap(),
        0,
    )
}

fn seat(room: &mut Room<u32>, n: u8) -> SeatId {
    match room.join(name(&format!("p{n}")), token(n)).unwrap() {
        JoinOutcome::Seated(id) => id,
        JoinOutcome::Queued(_) => panic!("queued"),
    }
}

#[test]
fn the_first_human_hosts_and_teams_fill_evenly() {
    let mut room = room(4, 2, 2, Admission::Open);
    let a = seat(&mut room, 1);
    let b = seat(&mut room, 2);
    let c = seat(&mut room, 3);
    assert_eq!(room.host(), Some(a));
    let teams: Vec<_> = [a, b, c]
        .map(|id| room.seat(id).unwrap().team().index())
        .to_vec();
    assert_eq!(teams, vec![0, 1, 0]);
    assert_eq!(room.set_team(c, c, TeamId::new(1)), Ok(()));
    assert_eq!(
        room.set_team(a, a, TeamId::new(1)),
        Err(SessionError::TeamFull)
    );
    assert_eq!(
        room.set_team(a, a, TeamId::new(2)),
        Err(SessionError::UnknownTeam)
    );
    assert_eq!(
        room.set_team(a, b, TeamId::new(0)),
        Err(SessionError::NotHost)
    );
    seat(&mut room, 4);
    assert_eq!(room.join(name("p5"), token(5)), Err(SessionError::RoomFull));
    assert_eq!(
        room.join(name("again"), token(1)),
        Err(SessionError::AlreadyInRoom)
    );
}

#[test]
fn the_match_starts_after_the_countdown_and_unready_cancels_it() {
    let mut room = room(4, 1, 2, Admission::Open);
    let a = seat(&mut room, 1);
    let b = seat(&mut room, 2);
    assert_eq!(room.start(a, Tick::ZERO), Err(SessionError::NotAllReady));
    room.set_ready(a, true).unwrap();
    room.set_ready(b, true).unwrap();
    assert_eq!(room.start(b, Tick::ZERO), Err(SessionError::NotHost));
    room.start(a, Tick::new(5)).unwrap();
    assert_eq!(
        room.phase(),
        Phase::Countdown {
            ends_at: Tick::new(15)
        }
    );

    room.set_ready(b, false).unwrap();
    assert_eq!(room.phase(), Phase::Lobby);
    assert!(
        room.drain_events()
            .any(|event| event == RoomEvent::CountdownCancelled)
    );

    room.set_ready(b, true).unwrap();
    room.start(a, Tick::new(20)).unwrap();
    room.tick(Tick::new(29));
    assert!(matches!(room.phase(), Phase::Countdown { .. }));
    room.tick(Tick::new(30));
    assert_eq!(room.phase(), Phase::InMatch);
    assert_eq!(room.set_ready(a, false), Err(SessionError::WrongPhase));
}

#[test]
fn a_mid_match_drop_keeps_the_seat_for_resume_and_moves_the_host() {
    let mut room = room(4, 1, 1, Admission::Open);
    let a = seat(&mut room, 1);
    let b = seat(&mut room, 2);
    room.set_ready(a, true).unwrap();
    room.set_ready(b, true).unwrap();
    room.start(a, Tick::ZERO).unwrap();
    room.tick(Tick::new(10));
    assert_eq!(room.phase(), Phase::InMatch);

    room.disconnect(a).unwrap();
    assert_eq!(
        room.seat(a).unwrap().occupant(),
        Occupant::Human { connected: false }
    );
    assert_eq!(room.host(), Some(b));
    assert_eq!(room.resume(&token(1)), Ok(a));
    assert_eq!(room.host(), Some(b), "the host stays where it moved");
    assert_eq!(room.resume(&token(9)), Err(SessionError::UnknownToken));

    room.disconnect(b).unwrap();
    room.end_match().unwrap();
    room.return_to_lobby().unwrap();
    assert!(room.seat(b).is_none(), "nobody resumed b's seat");
    assert_eq!(room.host(), Some(a));
    assert!(!room.seat(a).unwrap().ready());
}

#[test]
fn a_lobby_drop_frees_the_seat() {
    let mut room = room(4, 1, 1, Admission::Open);
    let a = seat(&mut room, 1);
    let b = seat(&mut room, 2);
    room.disconnect(a).unwrap();
    assert!(room.seat(a).is_none());
    assert_eq!(room.host(), Some(b));
    room.leave(b).unwrap();
    assert_eq!(room.host(), None);
    assert!(room.seats().is_empty());
}

#[test]
fn approval_queues_joiners_until_the_host_decides() {
    let mut room = room(3, 1, 1, Admission::Approval);
    let host = seat(&mut room, 1);
    let JoinOutcome::Queued(p2) = room.join(name("p2"), token(2)).unwrap() else {
        panic!("not queued");
    };
    let JoinOutcome::Queued(p3) = room.join(name("p3"), token(3)).unwrap() else {
        panic!("not queued");
    };
    assert_eq!(
        room.join(name("dup"), token(2)),
        Err(SessionError::AlreadyInRoom)
    );
    let b = room.approve(host, p2).unwrap();
    assert_eq!(room.approve(b, p3), Err(SessionError::NotHost));
    room.deny(host, p3).unwrap();
    assert_eq!(room.approve(host, p3), Err(SessionError::UnknownPending));
    assert_eq!(room.view().waiting, vec![]);

    room.set_admission(host, Admission::Closed).unwrap();
    assert_eq!(room.join(name("p4"), token(4)), Err(SessionError::Closed));
}

#[test]
fn the_host_manages_bots_and_kicks() {
    let mut room = room(4, 2, 4, Admission::Open);
    let a = seat(&mut room, 1);
    let b = seat(&mut room, 2);
    let bot = room.add_bot(a, name("bot")).unwrap();
    assert!(room.seat(bot).unwrap().ready());
    assert_eq!(room.add_bot(b, name("bot")), Err(SessionError::NotHost));
    assert_eq!(room.set_ready(bot, false), Err(SessionError::WrongOccupant));
    assert_eq!(room.leave(bot), Err(SessionError::WrongOccupant));
    assert_eq!(room.kick(a, a), Err(SessionError::CannotKickSelf));
    room.kick(a, b).unwrap();
    room.kick(a, bot).unwrap();
    assert_eq!(room.seats().len(), 1);
    assert_eq!(room.set_settings(a, 7), Ok(()));
    assert_eq!(*room.settings(), 7);
    assert_eq!(
        room.start(a, Tick::ZERO),
        Err(SessionError::NotEnoughPlayers)
    );
}

#[test]
fn views_carry_no_secrets() {
    let mut room = room(2, 1, 1, Admission::Open);
    room.join(name("ada"), token(0xAB)).unwrap();
    let bytes = postcard::to_allocvec(&room.view()).unwrap();
    assert!(
        !bytes.windows(4).any(|window| window == [0xAB; 4]),
        "token bytes leaked into the view"
    );
    assert!(format!("{room:?}").contains("ResumeToken(..)"));
}

/// A random command; ids index into what currently exists so most commands hit something.
#[derive(Debug, Clone)]
enum Op {
    Join(u8),
    Resume(u8),
    Disconnect(u8),
    Leave(u8),
    Withdraw(u8),
    Approve(u8, u8),
    Deny(u8, u8),
    Kick(u8, u8),
    AddBot(u8),
    SetTeam(u8, u8, u8),
    SetReady(u8, bool),
    SetAdmission(u8, u8),
    Start(u8),
    Cancel(u8),
    Tick,
    EndMatch,
    ReturnToLobby,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0_u8..12).prop_map(Op::Join),
        1 => (0_u8..12).prop_map(Op::Resume),
        1 => any::<u8>().prop_map(Op::Disconnect),
        1 => any::<u8>().prop_map(Op::Leave),
        1 => any::<u8>().prop_map(Op::Withdraw),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::Approve(a, b)),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::Deny(a, b)),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(a, b)| Op::Kick(a, b)),
        1 => any::<u8>().prop_map(Op::AddBot),
        1 => (any::<u8>(), any::<u8>(), 0_u8..4).prop_map(|(a, b, t)| Op::SetTeam(a, b, t)),
        2 => (any::<u8>(), any::<bool>()).prop_map(|(a, r)| Op::SetReady(a, r)),
        1 => (any::<u8>(), 0_u8..3).prop_map(|(a, m)| Op::SetAdmission(a, m)),
        2 => any::<u8>().prop_map(Op::Start),
        1 => any::<u8>().prop_map(Op::Cancel),
        2 => Just(Op::Tick),
        1 => Just(Op::EndMatch),
        1 => Just(Op::ReturnToLobby),
    ]
}

/// The `n`th seat, wrapping; callers skip commands that need a seat while the room is empty.
fn pick(room: &Room<u32>, n: u8) -> SeatId {
    let seats = room.seats();
    seats[usize::from(n) % seats.len()].id()
}

const fn needs_seat(op: &Op) -> bool {
    !matches!(
        op,
        Op::Join(_) | Op::Resume(_) | Op::Withdraw(_) | Op::Tick | Op::EndMatch | Op::ReturnToLobby
    )
}

fn check_invariants(room: &Room<u32>) {
    let config = room.config();
    let seats = room.seats();
    assert!(seats.len() <= usize::from(config.max_seats()));
    assert!(seats.windows(2).all(|pair| pair[0].id() < pair[1].id()));
    for team in 0..config.teams() {
        let count = seats.iter().filter(|s| s.team().index() == team).count();
        assert!(count <= usize::from(config.team_capacity()));
    }
    assert!(seats.iter().all(|s| s.team().index() < config.teams()));
    let humans: Vec<_> = seats
        .iter()
        .filter(|s| matches!(s.occupant(), Occupant::Human { .. }))
        .collect();
    let any_connected = humans
        .iter()
        .any(|s| s.occupant() == Occupant::Human { connected: true });
    match room.host() {
        None => assert!(humans.is_empty()),
        Some(host) => {
            let host = room.seat(host).unwrap();
            assert!(matches!(host.occupant(), Occupant::Human { .. }));
            if any_connected {
                assert_eq!(host.occupant(), Occupant::Human { connected: true });
            }
        }
    }
    if matches!(room.phase(), Phase::Countdown { .. }) {
        assert!(room.can_start());
        assert!(room.host().is_some());
    }
    if matches!(room.phase(), Phase::Lobby | Phase::Countdown { .. }) {
        assert!(
            humans
                .iter()
                .all(|s| s.occupant() == Occupant::Human { connected: true })
        );
    }
    let view = room.view();
    let mut tokens_seen = std::collections::HashSet::new();
    for n in 0..12_u8 {
        if room.seat_of(&token(n)).is_some() {
            assert!(tokens_seen.insert(n));
        }
    }
    assert_eq!(view.seats.len(), seats.len());
    assert!(view.waiting.len() <= usize::from(config.max_seats()));
}

proptest! {
    #[test]
    fn every_command_sequence_keeps_the_room_invariants(
        seats in 1_u8..9,
        teams in 1_u8..4,
        min in 1_u8..4,
        ops in proptest::collection::vec(op(), 1..120),
    ) {
        let teams = teams.min(seats);
        let min = min.min(seats);
        let mut room = Room::new(RoomConfig::new(seats, teams, min, Admission::Open, 3).unwrap(), 0_u32);
        let mut now = Tick::ZERO;
        let mut pending = Vec::new();
        for op in ops {
            if room.seats().is_empty() && needs_seat(&op) {
                continue;
            }
            let _refusals_are_expected = match op {
                Op::Join(n) => room.join(name("p"), token(n)).map(|outcome| {
                    if let JoinOutcome::Queued(id) = outcome {
                        pending.push(id);
                    }
                }),
                Op::Resume(n) => room.resume(&token(n)).map(drop),
                Op::Disconnect(n) => room.disconnect(pick(&room, n)),
                Op::Leave(n) => room.leave(pick(&room, n)),
                Op::Withdraw(n) => match pending.get(usize::from(n) % pending.len().max(1)) {
                    Some(id) => room.withdraw(*id),
                    None => Ok(()),
                },
                Op::Approve(a, n) | Op::Deny(a, n) if pending.is_empty() => {
                    let _ = (a, n);
                    Ok(())
                }
                Op::Approve(a, n) => {
                    room.approve(pick(&room, a), pending[usize::from(n) % pending.len()]).map(drop)
                }
                Op::Deny(a, n) => room.deny(pick(&room, a), pending[usize::from(n) % pending.len()]),
                Op::Kick(a, b) => room.kick(pick(&room, a), pick(&room, b)),
                Op::AddBot(a) => room.add_bot(pick(&room, a), name("bot")).map(drop),
                Op::SetTeam(a, b, t) => room.set_team(pick(&room, a), pick(&room, b), TeamId::new(t)),
                Op::SetReady(a, ready) => room.set_ready(pick(&room, a), ready),
                Op::SetAdmission(a, m) => room.set_admission(
                    pick(&room, a),
                    [Admission::Open, Admission::Approval, Admission::Closed][usize::from(m)],
                ),
                Op::Start(a) => room.start(pick(&room, a), now),
                Op::Cancel(a) => room.cancel_start(pick(&room, a)),
                Op::Tick => {
                    now = now.next();
                    room.tick(now);
                    Ok(())
                }
                Op::EndMatch => room.end_match(),
                Op::ReturnToLobby => room.return_to_lobby(),
            };
            check_invariants(&room);
            room.drain_events().for_each(drop);
        }
    }
}
