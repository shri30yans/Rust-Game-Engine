//! The lobby protocol and the server side that speaks it.
//!
//! [`LobbyRequest`] and [`LobbyReply`] are the messages a client and server exchange about a
//! room (sent on the reliable channel by whoever owns the transport). [`LobbyHost`] is the
//! server side for one room: it maps each connection `C` to the seat or queue place it holds,
//! turns requests into [`Room`] commands with the requester as the actor (a client can never
//! act for another seat), and writes the replies into an outbox.
//!
//! Invariants: a connection maps to at most one member and a member to one connection; after
//! any request that changed the room every member gets the new [`RoomView`], and a request
//! that changed nothing sends nothing to anyone else; refusals go to the requester only.

use serde::{Deserialize, Serialize};

use crate::codes::ResumeToken;
use crate::error::SessionError;
use crate::name::PlayerName;
use crate::room::{Admission, JoinOutcome, PendingId, Room, RoomEvent, RoomView, SeatId, TeamId};
use engine_core::Tick;

/// Where fresh resume tokens come from, so the host stays testable without OS randomness.
pub trait TokenSource {
    /// A new, unguessable token.
    ///
    /// # Errors
    /// [`SessionError::Entropy`] when no randomness is available.
    fn next_token(&mut self) -> Result<ResumeToken, SessionError>;
}

/// Tokens from the operating system's secure random source.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default, Clone, Copy)]
pub struct OsTokens;

#[cfg(not(target_arch = "wasm32"))]
impl TokenSource for OsTokens {
    fn next_token(&mut self) -> Result<ResumeToken, SessionError> {
        ResumeToken::generate()
    }
}

/// What a client asks of the room. The actor is always the sender's own seat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LobbyRequest<S> {
    /// Join under a name, or take a seat back with a saved token.
    Hello {
        /// The display name (ignored on a successful resume).
        name: PlayerName,
        /// A token from an earlier [`LobbyReply::Welcome`], to resume that seat.
        resume: Option<ResumeToken>,
    },
    /// Move to a team (or, as host, move a bot).
    SetTeam {
        /// The seat to move.
        seat: SeatId,
        /// The team to move to.
        team: TeamId,
    },
    /// Set the sender's ready flag.
    SetReady(bool),
    /// Host: change the game settings.
    SetSettings(S),
    /// Host: change who may join.
    SetAdmission(Admission),
    /// Host: start the countdown.
    Start,
    /// Host: stop the countdown.
    CancelStart,
    /// Host: remove a seat.
    Kick(SeatId),
    /// Host: seat a queued joiner.
    Approve(PendingId),
    /// Host: turn a queued joiner away.
    Deny(PendingId),
    /// Host: add a bot.
    AddBot(PlayerName),
    /// Give up the seat or queue place.
    Leave,
}

/// Where a welcomed client stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Membership {
    /// The client holds this seat.
    Seated(SeatId),
    /// The client waits for the host's approval.
    Waiting(PendingId),
}

/// What the server tells one client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LobbyReply<S> {
    /// The hello worked: keep the token to resume after a drop.
    Welcome {
        /// The secret that takes this seat back.
        token: ResumeToken,
        /// The seat or queue place.
        membership: Membership,
    },
    /// The host approved the client; it now holds this seat.
    Approved(SeatId),
    /// The room changed; this is all of it.
    Room(RoomView<S>),
    /// The request was refused; nothing changed.
    Refused(SessionError),
    /// The client lost its place: kicked, denied, or left. The server forgets the connection.
    Removed,
}

/// The server side of one room's lobby. `C` is the transport's connection id.
#[derive(Debug)]
pub struct LobbyHost<C, S, T> {
    room: Room<S>,
    members: Vec<(C, Membership)>,
    tokens: T,
    events: Vec<RoomEvent>,
}

impl<C: Copy + Eq, S: Clone, T: TokenSource> LobbyHost<C, S, T> {
    /// A host for `room`, drawing tokens from `tokens`.
    pub fn new(room: Room<S>, tokens: T) -> Self {
        let seats = usize::from(room.config().max_seats());
        Self {
            room,
            members: Vec::with_capacity(seats * 2),
            tokens,
            events: Vec::with_capacity(16),
        }
    }

    /// The room.
    #[must_use]
    pub const fn room(&self) -> &Room<S> {
        &self.room
    }

    /// The seat a connection holds.
    #[must_use]
    pub fn seat_of(&self, conn: C) -> Option<SeatId> {
        match self.membership(conn) {
            Some(Membership::Seated(seat)) => Some(seat),
            Some(Membership::Waiting(_)) | None => None,
        }
    }

    /// The connection holding a seat.
    #[must_use]
    pub fn conn_of(&self, seat: SeatId) -> Option<C> {
        self.members
            .iter()
            .find(|(_, member)| *member == Membership::Seated(seat))
            .map(|(conn, _)| *conn)
    }

    /// Applies one request from `conn` and writes the replies into `out`.
    pub fn on_request(
        &mut self,
        conn: C,
        request: LobbyRequest<S>,
        now: Tick,
        out: &mut Vec<(C, LobbyReply<S>)>,
    ) {
        match self.apply(conn, request, now, out) {
            Ok(()) => self.publish(out),
            Err(error) => out.push((conn, LobbyReply::Refused(error))),
        }
    }

    /// A connection closed: its seat is freed (lobby) or kept for a resume (match), and a
    /// queue place is given up.
    pub fn on_disconnect(&mut self, conn: C, out: &mut Vec<(C, LobbyReply<S>)>) {
        let Some(index) = self.members.iter().position(|(own, _)| *own == conn) else {
            return;
        };
        let (_, membership) = self.members.swap_remove(index);
        let _already_gone_is_fine = match membership {
            Membership::Seated(seat) => self.room.disconnect(seat),
            Membership::Waiting(pending) => self.room.withdraw(pending),
        };
        self.publish(out);
    }

    /// Advances time (the countdown) and tells everyone if the room changed.
    pub fn tick(&mut self, now: Tick, out: &mut Vec<(C, LobbyReply<S>)>) {
        self.room.tick(now);
        self.publish(out);
    }

    /// The game reports the match is over.
    ///
    /// # Errors
    /// [`SessionError::WrongPhase`] unless a match is running.
    pub fn end_match(&mut self, out: &mut Vec<(C, LobbyReply<S>)>) -> Result<(), SessionError> {
        self.room.end_match()?;
        self.publish(out);
        Ok(())
    }

    /// Back to the lobby after the results.
    ///
    /// # Errors
    /// [`SessionError::WrongPhase`] unless showing results.
    pub fn return_to_lobby(
        &mut self,
        out: &mut Vec<(C, LobbyReply<S>)>,
    ) -> Result<(), SessionError> {
        self.room.return_to_lobby()?;
        self.publish(out);
        Ok(())
    }

    fn membership(&self, conn: C) -> Option<Membership> {
        self.members
            .iter()
            .find(|(own, _)| *own == conn)
            .map(|(_, membership)| *membership)
    }

    fn seat(&self, conn: C) -> Result<SeatId, SessionError> {
        self.seat_of(conn).ok_or(SessionError::UnknownSeat)
    }

    fn apply(
        &mut self,
        conn: C,
        request: LobbyRequest<S>,
        now: Tick,
        out: &mut Vec<(C, LobbyReply<S>)>,
    ) -> Result<(), SessionError> {
        match request {
            LobbyRequest::Hello { name, resume } => self.hello(conn, name, resume, out),
            LobbyRequest::SetTeam { seat, team } => {
                self.room.set_team(self.seat(conn)?, seat, team)
            }
            LobbyRequest::SetReady(ready) => self.room.set_ready(self.seat(conn)?, ready),
            LobbyRequest::SetSettings(settings) => {
                self.room.set_settings(self.seat(conn)?, settings)
            }
            LobbyRequest::SetAdmission(admission) => {
                self.room.set_admission(self.seat(conn)?, admission)
            }
            LobbyRequest::Start => self.room.start(self.seat(conn)?, now),
            LobbyRequest::CancelStart => self.room.cancel_start(self.seat(conn)?),
            LobbyRequest::Kick(target) => self.room.kick(self.seat(conn)?, target),
            LobbyRequest::Approve(pending) => {
                let seat = self.room.approve(self.seat(conn)?, pending)?;
                if let Some(member) = self
                    .members
                    .iter_mut()
                    .find(|(_, member)| *member == Membership::Waiting(pending))
                {
                    member.1 = Membership::Seated(seat);
                    out.push((member.0, LobbyReply::Approved(seat)));
                }
                Ok(())
            }
            LobbyRequest::Deny(pending) => self.room.deny(self.seat(conn)?, pending),
            LobbyRequest::AddBot(name) => self.room.add_bot(self.seat(conn)?, name).map(drop),
            LobbyRequest::Leave => {
                match self.membership(conn).ok_or(SessionError::UnknownSeat)? {
                    Membership::Seated(seat) => self.room.leave(seat)?,
                    Membership::Waiting(pending) => self.room.withdraw(pending)?,
                }
                Ok(())
            }
        }
    }

    fn hello(
        &mut self,
        conn: C,
        name: PlayerName,
        resume: Option<ResumeToken>,
        out: &mut Vec<(C, LobbyReply<S>)>,
    ) -> Result<(), SessionError> {
        if self.membership(conn).is_some() {
            return Err(SessionError::AlreadyInRoom);
        }
        if let Some(token) = resume
            && let Ok(seat) = self.room.resume(&token)
        {
            // The old connection, if the server has not noticed it drop yet, loses the seat.
            self.members
                .retain(|(_, member)| *member != Membership::Seated(seat));
            return self.welcome(conn, token, Membership::Seated(seat), out);
        }
        let token = self.tokens.next_token()?;
        let membership = match self.room.join(name, token)? {
            JoinOutcome::Seated(seat) => Membership::Seated(seat),
            JoinOutcome::Queued(pending) => Membership::Waiting(pending),
        };
        self.welcome(conn, token, membership, out)
    }

    fn welcome(
        &mut self,
        conn: C,
        token: ResumeToken,
        membership: Membership,
        out: &mut Vec<(C, LobbyReply<S>)>,
    ) -> Result<(), SessionError> {
        self.members.push((conn, membership));
        out.push((conn, LobbyReply::Welcome { token, membership }));
        Ok(())
    }

    /// Drops members the room removed, then sends the new view to everyone left if anything
    /// changed.
    fn publish(&mut self, out: &mut Vec<(C, LobbyReply<S>)>) {
        self.events.clear();
        self.events.extend(self.room.drain_events());
        if self.events.is_empty() {
            return;
        }
        for event in &self.events {
            let gone = match *event {
                RoomEvent::Left(seat) | RoomEvent::Kicked(seat) => Membership::Seated(seat),
                RoomEvent::Withdrawn(pending) => Membership::Waiting(pending),
                _ => continue,
            };
            if let Some(index) = self.members.iter().position(|(_, member)| *member == gone) {
                let (conn, _) = self.members.swap_remove(index);
                out.push((conn, LobbyReply::Removed));
            }
        }
        let view = self.room.view();
        for (conn, _) in &self.members {
            out.push((*conn, LobbyReply::Room(view.clone())));
        }
    }
}
