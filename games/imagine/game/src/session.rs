//! The local room that owns lobby seats, countdown and match travel settings.
//!
//! Invariants: one local human is the room host; room changes use engine-sessions commands; all
//! timing uses simulation ticks, and bot settings are frozen into the room before it starts.

use engine_core::{SimRng, Tick};
use engine_sessions::{
    Admission, JoinOutcome, Occupant, Phase, PlayerName, ResumeToken, Room, RoomCode, RoomConfig,
    RoomEvent, SeatId, TeamId,
};
use thiserror::Error;

use crate::data::{ModeCapacity, ModeDefinition};
use crate::sim::{ImagineMode, ImagineSettings};

/// Room countdown duration at IMAGINE's 60 Hz simulation rate.
pub const COUNTDOWN_TICKS: u32 = 180;
/// Current Practice round length: ten seconds until a mode rule supplies a real objective.
pub const PRACTICE_MATCH_TICKS: u32 = 600;
const ROOM_TEAMS: u8 = 2;

/// Errors returned by local-room setup.
#[derive(Debug, Error)]
pub enum LocalSessionError {
    /// Room configuration or seat operation was refused.
    #[error(transparent)]
    Session(#[from] engine_sessions::SessionError),
    /// The selected room mode cannot run in this build.
    #[error("the selected mode is not playable")]
    UnsupportedMode,
    /// The configured open room failed to seat its first human.
    #[error("the local room did not seat its host")]
    LocalPlayerNotSeated,
    /// The selected mode has no free bot slot.
    #[error("the room has reached this mode's player capacity")]
    BotCapacityReached,
    /// The configured roster has no unused name for the next bot.
    #[error("the bot roster has no available name")]
    BotRosterExhausted,
    /// The selected mode is unavailable in this build.
    #[error("the selected mode is unavailable")]
    ModeUnavailable,
}

/// Signals emitted when the room changes phase during a fixed tick.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionUpdate {
    /// The countdown started.
    pub countdown_started: bool,
    /// An input cancelled the countdown.
    pub countdown_cancelled: bool,
    /// Match travel settings are ready.
    pub match_started: bool,
    /// The room entered results.
    pub match_ended: bool,
    /// A room command failed while the session advanced.
    pub failure: Option<String>,
}

/// One local session room and the host's seat.
pub struct LocalSession {
    room: Room<ImagineSettings>,
    host: SeatId,
    code: RoomCode,
    tick: Tick,
    match_ticks: u32,
    bot_roster: Vec<String>,
    capacity: ModeCapacity,
}

impl LocalSession {
    /// Builds an open room using the selected mode's capacity and seats the host.
    ///
    /// # Errors
    ///
    /// Returns a typed sessions error if the room config or local name is invalid.
    pub fn new(
        settings: ImagineSettings,
        seed: u64,
        mode: &ModeDefinition,
        bot_roster: &[String],
    ) -> Result<Self, LocalSessionError> {
        mode.capacity
            .validate()
            .map_err(|_| LocalSessionError::ModeUnavailable)?;
        if !mode.available {
            return Err(LocalSessionError::ModeUnavailable);
        }
        let required_names = usize::from(mode.capacity.max_bots());
        if bot_roster.len() < required_names {
            return Err(LocalSessionError::BotRosterExhausted);
        }
        let config = RoomConfig::new(
            mode.capacity.max_players,
            ROOM_TEAMS,
            mode.capacity.min_players,
            Admission::Open,
            COUNTDOWN_TICKS,
        )?;
        let mut room = Room::new(config, settings);
        let seed_bytes = seed.to_le_bytes();
        let token = ResumeToken::from_bytes(std::array::from_fn(|index| {
            seed_bytes.get(index % 8).copied().unwrap_or_default()
        }));
        let host = match room.join(PlayerName::new("Player")?, token)? {
            JoinOutcome::Seated(id) => id,
            JoinOutcome::Queued(_) => return Err(LocalSessionError::LocalPlayerNotSeated),
        };
        let mut rng = SimRng::new(seed, 0);
        let code = RoomCode::generate(&mut rng);
        Ok(Self {
            room,
            host,
            code,
            tick: Tick::ZERO,
            match_ticks: 0,
            bot_roster: bot_roster.to_vec(),
            capacity: mode.capacity,
        })
    }

    /// Room address displayed in the lobby.
    #[must_use]
    pub const fn code(&self) -> RoomCode {
        self.code
    }

    /// Room phase.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.room.phase()
    }

    /// Room view for menu bindings.
    #[must_use]
    pub fn view(&self) -> engine_sessions::RoomView<ImagineSettings> {
        self.room.view()
    }

    /// Whether this mode's room can accept another bot.
    #[must_use]
    pub fn can_add_bot(&self) -> bool {
        self.room.seats().len() < usize::from(self.capacity.max_players)
    }

    /// Whether the host can remove at least one bot.
    #[must_use]
    pub fn can_remove_bot(&self) -> bool {
        self.room
            .seats()
            .iter()
            .any(|seat| matches!(seat.occupant(), Occupant::Bot))
    }

    /// Minimum number of seats required to start this mode.
    #[must_use]
    pub const fn min_players(&self) -> u8 {
        self.capacity.min_players
    }

    /// Maximum number of players in this mode.
    #[must_use]
    pub const fn max_players(&self) -> u8 {
        self.capacity.max_players
    }

    /// Adds a bot as the host.
    ///
    /// # Errors
    ///
    /// Returns the room refusal if the room is full or is not in a lobby phase.
    pub fn add_bot(&mut self) -> Result<SeatId, LocalSessionError> {
        if !self.can_add_bot() {
            return Err(LocalSessionError::BotCapacityReached);
        }
        let used_names = self
            .room
            .seats()
            .iter()
            .filter(|seat| matches!(seat.occupant(), Occupant::Bot))
            .map(|seat| seat.name().as_str())
            .collect::<std::collections::HashSet<_>>();
        let name = self
            .bot_roster
            .iter()
            .find(|name| !used_names.contains(name.as_str()))
            .ok_or(LocalSessionError::BotRosterExhausted)?;
        let name = PlayerName::new(name)?;
        Ok(self.room.add_bot(self.host, name)?)
    }

    /// Removes a bot at a row index.
    ///
    /// # Errors
    ///
    /// Returns the room refusal if the row is absent or names a human.
    pub fn remove_bot_at(&mut self, row: usize) -> Result<(), LocalSessionError> {
        let view = self.room.view();
        let seat = view
            .seats
            .get(row)
            .ok_or(engine_sessions::SessionError::UnknownSeat)?;
        if !matches!(seat.occupant, Occupant::Bot) {
            return Err(engine_sessions::SessionError::WrongOccupant.into());
        }
        self.room.kick(self.host, seat.id)?;
        Ok(())
    }

    /// Moves a seat to the next team.
    ///
    /// # Errors
    ///
    /// Returns the room refusal when the team is full or the row is absent.
    pub fn cycle_team_at(&mut self, row: usize) -> Result<(), LocalSessionError> {
        let view = self.room.view();
        let seat = view
            .seats
            .get(row)
            .ok_or(engine_sessions::SessionError::UnknownSeat)?;
        let next = (seat.team.index() + 1) % view.config.teams();
        self.room.set_team(self.host, seat.id, TeamId::new(next))?;
        Ok(())
    }

    /// Toggles the local human's ready flag.
    ///
    /// # Errors
    ///
    /// Returns the room refusal when the room is already in a match.
    pub fn toggle_ready(&mut self) -> Result<(), LocalSessionError> {
        let ready = self.room.seat(self.host).is_some_and(|seat| !seat.ready());
        self.room.set_ready(self.host, ready)?;
        Ok(())
    }

    /// Starts the room countdown after freezing its selected bots and teams into settings.
    ///
    /// # Errors
    ///
    /// Returns the room refusal when the host is not ready or the minimum seat count is unmet.
    pub fn start(&mut self) -> Result<(), LocalSessionError> {
        let settings = match self.room.settings().mode {
            ImagineMode::Practice => self.match_settings()?,
        };
        self.room.set_settings(self.host, settings)?;
        self.room.start(self.host, self.tick)?;
        Ok(())
    }

    /// Advances the room by one deterministic simulation tick and reports phase events.
    pub fn tick(&mut self) -> SessionUpdate {
        let mut update = SessionUpdate::default();
        self.tick = self.tick.next();
        if self.room.phase() == Phase::InMatch {
            self.match_ticks = self.match_ticks.saturating_add(1);
            if self.match_ticks >= PRACTICE_MATCH_TICKS
                && let Err(error) = self.room.end_match()
            {
                update.failure = Some(error.to_string());
            }
        }
        self.room.tick(self.tick);
        for event in self.room.drain_events() {
            match event {
                RoomEvent::CountdownStarted(_) => update.countdown_started = true,
                RoomEvent::CountdownCancelled => update.countdown_cancelled = true,
                RoomEvent::MatchStarted => {
                    update.match_started = true;
                    self.match_ticks = 0;
                }
                RoomEvent::MatchEnded => update.match_ended = true,
                _ => {}
            }
        }
        update
    }

    /// Returns the room to its lobby after results.
    ///
    /// # Errors
    ///
    /// Returns WrongPhase unless the room currently shows results.
    pub fn return_to_lobby(&mut self) -> Result<(), LocalSessionError> {
        self.room.return_to_lobby()?;
        Ok(())
    }

    fn match_settings(&self) -> Result<ImagineSettings, LocalSessionError> {
        let view = self.room.view();
        let bot_teams = view
            .seats
            .iter()
            .filter(|seat| matches!(seat.occupant, Occupant::Bot))
            .map(|seat| seat.team.index())
            .collect::<Vec<_>>();
        Ok(ImagineSettings {
            arena_id: view.settings.arena_id,
            mode: view.settings.mode,
            menu_backdrop: false,
            bot_teams,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn room() -> LocalSession {
        let content = crate::test_support::pack().expect("test pack");
        let path = engine_assets::AssetPath::new("data/modes.ron").expect("mode path");
        let modes = crate::data::mode_definitions(content.load_text(&path).expect("mode data"))
            .expect("valid modes");
        let mode = modes
            .iter()
            .find(|mode| mode.id == "practice")
            .expect("practice mode");
        let tuning = crate::data::load_tuning(&content).expect("valid tuning");
        LocalSession::new(
            ImagineSettings::default(),
            7,
            mode,
            &tuning.actors.bot_roster,
        )
        .expect("valid local room")
    }

    #[test]
    fn adds_and_removes_bots_without_exceeding_seat_capacity() {
        let mut room = room();
        for _ in 0..6 {
            room.add_bot().expect("room has a free seat");
        }
        assert!(!room.can_add_bot());
        assert!(matches!(
            room.add_bot(),
            Err(LocalSessionError::BotCapacityReached)
        ));
        room.remove_bot_at(6).expect("last seat is a bot");
        assert_eq!(room.view().seats.len(), 6);
    }

    #[test]
    fn every_available_mode_starts_with_its_maximum_bot_count() {
        let content = crate::test_support::pack().expect("test pack");
        let path = engine_assets::AssetPath::new("data/modes.ron").expect("mode path");
        let modes = crate::data::mode_definitions(content.load_text(&path).expect("mode data"))
            .expect("valid modes");
        let tuning = crate::data::load_tuning(&content).expect("valid tuning");
        let mut available = 0;
        for mode in modes.iter().filter(|mode| mode.available) {
            available += 1;
            let mut session = LocalSession::new(
                ImagineSettings::default(),
                91,
                mode,
                &tuning.actors.bot_roster,
            )
            .expect("available mode has enough bot names");
            for _ in 0..mode.capacity.max_bots() {
                session.add_bot().expect("mode has a bot seat");
            }
            assert!(!session.can_add_bot());
            session.toggle_ready().expect("host can ready");
            session.start().expect("full room starts");
            let mut started = false;
            for _ in 0..COUNTDOWN_TICKS {
                started |= session.tick().match_started;
            }
            assert!(started, "{} never started", mode.name);
            assert_eq!(
                session.view().settings.bot_teams.len(),
                usize::from(mode.capacity.max_bots()),
                "{} did not retain its full room roster",
                mode.name
            );
        }
        assert!(available > 0);
    }

    #[test]
    fn ready_gate_and_countdown_cancellation_follow_room_rules() {
        let mut room = room();
        room.add_bot().expect("bot seat");
        assert!(matches!(
            room.start(),
            Err(LocalSessionError::Session(
                engine_sessions::SessionError::NotAllReady
            ))
        ));
        room.toggle_ready().expect("ready");
        room.start().expect("ready player and bot start");
        assert_eq!(
            room.phase(),
            Phase::Countdown {
                ends_at: Tick::new(COUNTDOWN_TICKS)
            }
        );
        room.toggle_ready().expect("unready");
        assert_eq!(room.phase(), Phase::Lobby);
    }

    #[test]
    fn countdown_starts_match_with_room_bot_count_and_returns_humans_unready() {
        let mut room = room();
        room.add_bot().expect("bot seat");
        room.add_bot().expect("second bot seat");
        room.toggle_ready().expect("ready");
        room.start().expect("ready player and bots start");
        let mut started = false;
        for _ in 0..COUNTDOWN_TICKS {
            started |= room.tick().match_started;
        }
        assert!(started);
        assert_eq!(room.view().settings.bot_teams.len(), 2);
        for _ in 0..PRACTICE_MATCH_TICKS {
            let update = room.tick();
            if update.match_ended {
                break;
            }
        }
        assert_eq!(room.phase(), Phase::Results);
        room.return_to_lobby().expect("return to room");
        assert!(
            !room
                .view()
                .seats
                .iter()
                .find(|seat| seat.id == room.host)
                .is_some_and(|seat| seat.ready)
        );
    }
}
