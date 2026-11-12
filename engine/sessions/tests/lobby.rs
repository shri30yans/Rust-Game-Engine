//! The lobby host over fake connection ids: hello, resume, approval, kicks, the match cycle,
//! and that a client can only act as its own seat.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use engine_core::Tick;
use engine_sessions::{
    Admission, LobbyHost, LobbyReply, LobbyRequest, Membership, Occupant, Phase, PlayerName,
    ResumeToken, Room, RoomConfig, SeatId, SessionError, TokenSource,
};

/// Tokens 1, 2, 3...: deterministic and distinct.
struct Counter(u8);

impl TokenSource for Counter {
    fn next_token(&mut self) -> Result<ResumeToken, SessionError> {
        self.0 += 1;
        Ok(ResumeToken::from_bytes([self.0; 32]))
    }
}

type Host = LobbyHost<u32, u8, Counter>;
type Out = Vec<(u32, LobbyReply<u8>)>;

fn host(admission: Admission) -> Host {
    let config = RoomConfig::new(4, 2, 2, admission, 5).unwrap();
    LobbyHost::new(Room::new(config, 0_u8), Counter(0))
}

fn hello(name: &str, resume: Option<ResumeToken>) -> LobbyRequest<u8> {
    LobbyRequest::Hello {
        name: PlayerName::new(name).unwrap(),
        resume,
    }
}

/// Sends a hello and returns the welcome's token and membership.
fn join(host: &mut Host, conn: u32, name: &str, out: &mut Out) -> (ResumeToken, Membership) {
    out.clear();
    host.on_request(conn, hello(name, None), Tick::ZERO, out);
    match out.first() {
        Some((to, LobbyReply::Welcome { token, membership })) if *to == conn => {
            (*token, *membership)
        }
        other => panic!("no welcome: {other:?}"),
    }
}

fn seat(membership: Membership) -> SeatId {
    match membership {
        Membership::Seated(seat) => seat,
        Membership::Waiting(_) => panic!("waiting"),
    }
}

fn views_to(out: &Out) -> Vec<u32> {
    let mut to: Vec<u32> = out
        .iter()
        .filter(|(_, reply)| matches!(reply, LobbyReply::Room(_)))
        .map(|(conn, _)| *conn)
        .collect();
    to.sort_unstable();
    to
}

#[test]
fn joiners_are_welcomed_and_everyone_sees_each_change() {
    let mut host = host(Admission::Open);
    let mut out = Vec::new();
    let (_, a) = join(&mut host, 10, "ada", &mut out);
    assert_eq!(views_to(&out), vec![10]);
    let (_, b) = join(&mut host, 11, "bo", &mut out);
    assert_eq!(views_to(&out), vec![10, 11]);
    assert_eq!(host.seat_of(10), Some(seat(a)));
    assert_eq!(host.conn_of(seat(b)), Some(11));

    out.clear();
    host.on_request(11, LobbyRequest::SetReady(true), Tick::ZERO, &mut out);
    assert_eq!(views_to(&out), vec![10, 11]);

    out.clear();
    host.on_request(11, LobbyRequest::SetReady(true), Tick::ZERO, &mut out);
    assert!(out.is_empty(), "no change, no traffic");

    out.clear();
    host.on_request(11, LobbyRequest::Start, Tick::ZERO, &mut out);
    assert_eq!(out, vec![(11, LobbyReply::Refused(SessionError::NotHost))]);

    out.clear();
    host.on_request(10, hello("again", None), Tick::ZERO, &mut out);
    assert_eq!(
        out,
        vec![(10, LobbyReply::Refused(SessionError::AlreadyInRoom))]
    );

    out.clear();
    host.on_request(99, LobbyRequest::SetReady(true), Tick::ZERO, &mut out);
    assert_eq!(
        out,
        vec![(99, LobbyReply::Refused(SessionError::UnknownSeat))]
    );
}

#[test]
fn a_client_can_only_act_as_its_own_seat() {
    let mut host = host(Admission::Open);
    let mut out = Vec::new();
    let (_, a) = join(&mut host, 1, "ada", &mut out);
    join(&mut host, 2, "bo", &mut out);
    out.clear();
    host.on_request(2, LobbyRequest::Kick(seat(a)), Tick::ZERO, &mut out);
    assert_eq!(out, vec![(2, LobbyReply::Refused(SessionError::NotHost))]);
    assert!(host.room().seat(seat(a)).is_some());
}

#[test]
fn a_drop_mid_match_is_resumed_on_a_new_connection_with_the_token() {
    let mut host = host(Admission::Open);
    let mut out = Vec::new();
    let (token_a, a) = join(&mut host, 1, "ada", &mut out);
    join(&mut host, 2, "bo", &mut out);
    for conn in [1, 2] {
        host.on_request(conn, LobbyRequest::SetReady(true), Tick::ZERO, &mut out);
    }
    host.on_request(1, LobbyRequest::Start, Tick::ZERO, &mut out);
    host.tick(Tick::new(5), &mut out);
    assert_eq!(host.room().phase(), Phase::InMatch);

    out.clear();
    host.on_disconnect(1, &mut out);
    assert_eq!(views_to(&out), vec![2]);
    assert_eq!(
        host.room().seat(seat(a)).unwrap().occupant(),
        Occupant::Human { connected: false }
    );

    out.clear();
    host.on_request(7, hello("ada", Some(token_a)), Tick::new(6), &mut out);
    assert_eq!(
        out.first(),
        Some(&(
            7,
            LobbyReply::Welcome {
                token: token_a,
                membership: a
            }
        ))
    );
    assert_eq!(host.conn_of(seat(a)), Some(7));
    assert_eq!(views_to(&out), vec![2, 7]);
}

#[test]
fn a_resume_from_a_new_connection_takes_the_seat_from_a_stale_one() {
    let mut host = host(Admission::Open);
    let mut out = Vec::new();
    let (token, a) = join(&mut host, 1, "ada", &mut out);
    out.clear();
    host.on_request(2, hello("ada", Some(token)), Tick::ZERO, &mut out);
    assert_eq!(host.conn_of(seat(a)), Some(2));
    assert_eq!(host.seat_of(1), None);
    out.clear();
    host.on_disconnect(1, &mut out);
    assert!(
        out.is_empty(),
        "the stale connection no longer owns anything"
    );
    assert!(host.room().seat(seat(a)).is_some());
}

#[test]
fn approval_moves_a_waiting_connection_onto_a_seat_and_denial_removes_it() {
    let mut host = host(Admission::Approval);
    let mut out = Vec::new();
    join(&mut host, 1, "host", &mut out);
    let (_, waiting) = join(&mut host, 2, "bo", &mut out);
    let (_, denied) = join(&mut host, 3, "cy", &mut out);
    let (Membership::Waiting(p2), Membership::Waiting(p3)) = (waiting, denied) else {
        panic!("not queued");
    };
    assert_eq!(
        views_to(&out),
        vec![1, 2, 3],
        "waiting players see the room too"
    );

    out.clear();
    host.on_request(1, LobbyRequest::Approve(p2), Tick::ZERO, &mut out);
    let seat_b = host.seat_of(2).unwrap();
    assert!(out.contains(&(2, LobbyReply::Approved(seat_b))));

    out.clear();
    host.on_request(1, LobbyRequest::Deny(p3), Tick::ZERO, &mut out);
    assert!(out.contains(&(3, LobbyReply::Removed)));
    assert_eq!(views_to(&out), vec![1, 2]);
}

#[test]
fn kicked_and_leaving_players_are_told_and_forgotten() {
    let mut host = host(Admission::Open);
    let mut out = Vec::new();
    join(&mut host, 1, "host", &mut out);
    let (_, b) = join(&mut host, 2, "bo", &mut out);
    join(&mut host, 3, "cy", &mut out);

    out.clear();
    host.on_request(1, LobbyRequest::Kick(seat(b)), Tick::ZERO, &mut out);
    assert!(out.contains(&(2, LobbyReply::Removed)));
    assert_eq!(views_to(&out), vec![1, 3]);
    assert_eq!(host.seat_of(2), None);

    out.clear();
    host.on_request(3, LobbyRequest::Leave, Tick::ZERO, &mut out);
    assert!(out.contains(&(3, LobbyReply::Removed)));
    assert_eq!(views_to(&out), vec![1]);
}

#[test]
fn messages_round_trip_through_postcard() {
    let mut host = host(Admission::Open);
    let mut out = Vec::new();
    join(&mut host, 1, "ada", &mut out);
    for (_, reply) in &out {
        let bytes = postcard::to_allocvec(reply).unwrap();
        let back: LobbyReply<u8> = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(&back, reply);
    }
    let request = hello("ada", Some(ResumeToken::from_bytes([3; 32])));
    let bytes = postcard::to_allocvec(&request).unwrap();
    assert_eq!(
        postcard::from_bytes::<LobbyRequest<u8>>(&bytes).unwrap(),
        request
    );
}

#[test]
fn an_invalid_config_does_not_decode() {
    let good = RoomConfig::new(4, 2, 2, Admission::Open, 5).unwrap();
    let mut bytes = postcard::to_allocvec(&good).unwrap();
    bytes[1] = 9; // teams = 9
    assert!(postcard::from_bytes::<RoomConfig>(&bytes).is_err());
}
