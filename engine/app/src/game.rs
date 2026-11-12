//! The game registration contract.
//!
//! A game hands the app its identity and its systems. The app never reaches
//! back into the game, and nothing here names a game, a mode or a file path.

use engine_assets::ContentPack;
use engine_netcode::{EntityReplication, InputPayload, Predicted};
use engine_scene::{Level, SceneContent, SceneError};
use engine_sessions::{PlayerName, SeatId};
use engine_world::NetId;
use engine_world::{Schedule, World};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Identity of a game for the network handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameInfo {
    /// Human-readable game name, for logs and handshake errors.
    pub name: &'static str,
    /// Unique protocol id. A peer with a different id is refused.
    pub protocol_id: u32,
    /// Wire protocol version of this game.
    pub protocol_version: u16,
}

/// Identity of one human or bot in a room. Standalone play reserves `u32::MAX` for its local player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerId(SeatId);

impl PlayerId {
    /// The sole player in a standalone session.
    pub const LOCAL: Self = Self(SeatId::new(u32::MAX));

    /// Wraps a lobby seat as a game player identity.
    #[must_use]
    pub const fn from_seat(seat: SeatId) -> Self {
        Self(seat)
    }

    /// The session seat represented by this player.
    #[must_use]
    pub const fn seat(self) -> SeatId {
        self.0
    }
}

/// What a game hands the app. The engine never opens a game file by a fixed path.
pub trait Game {
    /// One tick of one player's intent.
    type Command: InputPayload;
    /// Settings selected in the lobby.
    type Settings: Clone + Serialize + DeserializeOwned + PartialEq + core::fmt::Debug;

    /// Canonical content root, prefab registry and collision policy for scene spawning.
    ///
    /// Games without cooked level content may use the empty primitive-only default.
    fn scene_content(&self) -> Result<SceneContent, SceneError> {
        Ok(SceneContent::empty())
    }

    /// Refreshes game-owned scene roles and metadata after the app accepts a live level edit.
    ///
    /// The app has already applied the stable-ID entity diff through [`Self::scene_content`].
    /// Implementations rebuild their derived data from the accepted [`Level`] and canonical
    /// spawned entries without travelling or reinstalling the level.
    fn scene_reloaded(
        &self,
        _world: &mut World,
        _level: &Level,
        _content: &ContentPack,
    ) -> Result<(), SceneError> {
        Ok(())
    }

    /// Maximum number of non-level entities this game can add to a loaded world.
    ///
    /// The client adds this budget to the initialized world's entity count when it sizes frame
    /// extraction storage. A level replacement refreshes that capacity from the resulting world.
    /// There is no default: every game states how many entities it spawns at run time.
    fn dynamic_entity_budget(&self) -> usize;

    /// Identity used by the network handshake.
    fn info(&self) -> GameInfo;

    /// Registers components and adds systems in update order.
    ///
    /// # Errors
    ///
    /// Returns any error from registering components or adding systems.
    fn setup(
        &self,
        world: &mut World,
        schedule: &mut Schedule,
        settings: &Self::Settings,
    ) -> Result<(), AppError>;

    /// Joins a player and returns its pawn, as in `PostLogin` / `RestartPlayer`.
    fn join(
        &self,
        world: &mut World,
        player: PlayerId,
        name: &PlayerName,
    ) -> Result<NetId, AppError>;

    /// Removes a player's pawn and associated game state.
    fn leave(&self, world: &mut World, player: PlayerId) -> Result<(), AppError>;

    /// Applies one tick of player intent. A missing or dead pawn ignores the command.
    fn command(
        &self,
        world: &mut World,
        player: PlayerId,
        command: &Self::Command,
    ) -> Result<(), AppError>;

    /// Adjusts lobby settings using the room's seated bot count before match setup.
    fn settings_for_match(
        &self,
        settings: &Self::Settings,
        _bot_count: usize,
    ) -> Result<Self::Settings, AppError> {
        Ok(settings.clone())
    }
}

/// The online game contract. Standalone-only games need not implement replication or prediction.
pub trait NetGame: Game {
    /// Client-side prediction behavior for this game's command.
    type Predicted: Predicted<Input = Self::Command>;

    /// Replication policy for one entity as seen by one player.
    fn replication(&self, world: &World, viewer: PlayerId, entity: NetId) -> EntityReplication;

    /// Transform bounds for the supplied match settings, validated before world travel.
    ///
    /// # Errors
    /// Returns an app error when those bounds cannot meet the game's network precision.
    fn transform_bounds(
        &self,
        settings: &Self::Settings,
    ) -> Result<engine_netcode::TransformBounds, AppError>;

    /// The protocol identity this game uses during the network handshake.
    fn protocol(&self) -> engine_netcode::GameProtocol;
}
