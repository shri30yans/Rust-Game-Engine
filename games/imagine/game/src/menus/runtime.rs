//! IMAGINE menu runtime state and binding accessors.

use std::collections::HashMap;

use app::{AppError, ClientAssets, MenuInputMapper};
use engine_assets::{AssetPath, ContentPack};
use engine_ui::{
    Bindings, Layer, ListBinding, MenuInput, MenuMotion, Menus, Theme, ThemeSerial, UiEvent,
};

use crate::data::mode_definitions;
use crate::flow::Screen;

use super::actions::MenuAction;
use super::bindings::display_number;
use super::documents::{MENU_SCREENS, document_path, document_source};

/// Owns menu documents and applies the game-agnostic menu runtime.
pub struct GameMenus {
    pub(super) runtime: Menus,
    pub(super) bindings: Bindings,
    pub(super) mapper: MenuInputMapper,
    pub(super) input: MenuInput,
    pub(super) active: Vec<ActiveScreen>,
    pub(super) events: Vec<MenuAction>,
    pub(super) raw_events: Vec<UiEvent>,
    pub(super) now_ms: u64,
    pub(super) last_screen: (f32, f32),
    pub(super) document_overrides: HashMap<Screen, String>,
    pub(super) document_versions: HashMap<Screen, u64>,
    pub(super) document_hashes: HashMap<Screen, u64>,
}

pub(super) struct ActiveScreen {
    pub(super) screen: Screen,
    pub(super) layer: Layer,
    pub(super) id: engine_ui::ScreenId,
    pub(super) document_version: u64,
}

impl GameMenus {
    /// Loads and validates all IMAGINE menu documents and starts on the boot screen.
    ///
    /// # Errors
    ///
    /// Returns a UI error for invalid theme tokens or documents.
    pub fn new(content: &ContentPack) -> Result<Self, AppError> {
        let theme_path = AssetPath::new("ui/theme.ron")?;
        let theme = Theme::from_ron(content.load_text(&theme_path)?, ThemeSerial(1))?;
        let motion = MenuMotion {
            screen: theme.motion(theme.motion_id("m-screen")?)?.clone(),
            modal: theme.motion(theme.motion_id("m-modal")?)?.clone(),
            exit_ms: 120,
            slide_px: 24.0,
        };
        let mode_path = AssetPath::new("data/modes.ron")?;
        let mode_data = mode_definitions(content.load_text(&mode_path)?)
            .map_err(|error| AppError::Ui(engine_ui::UiError::Parse(error)))?;
        let mut document_overrides = HashMap::new();
        let mut document_hashes = HashMap::new();
        for screen in MENU_SCREENS {
            let path = AssetPath::new(document_path(*screen))?;
            document_overrides.insert(*screen, document_source(content, *screen)?);
            document_hashes.insert(*screen, content.hash(&path)?);
        }
        let mut menus = Self {
            runtime: Menus::new(motion),
            bindings: Bindings::new(),
            mapper: MenuInputMapper::default(),
            input: MenuInput::new(),
            active: Vec::with_capacity(8),
            events: Vec::with_capacity(16),
            raw_events: Vec::with_capacity(32),
            now_ms: 0,
            last_screen: (0.0, 0.0),
            document_overrides,
            document_versions: HashMap::new(),
            document_hashes,
        };
        menus.bind_defaults(&mode_data);
        menus.push(Screen::Boot, Layer::Menu)?;
        Ok(menus)
    }

    pub(super) fn document_version(&self, screen: Screen) -> u64 {
        self.document_versions.get(&screen).copied().unwrap_or(0)
    }

    /// Reloads changed menu documents from the client's mounted content pack.
    pub fn reload_from_assets(&mut self, assets: &ClientAssets<'_>) -> Result<(), AppError> {
        for screen in MENU_SCREENS {
            let path = AssetPath::new(document_path(*screen))?;
            let hash = assets.content_hash(&path)?;
            if self.document_hashes.get(screen) == Some(&hash) {
                continue;
            }
            let text = assets.content_text(&path)?.to_owned();
            self.replace_document_source(*screen, text)?;
            self.document_hashes.insert(*screen, hash);
        }
        Ok(())
    }

    /// The active game-independent runtime, for accessibility motion updates.
    pub fn set_reduce_motion(&mut self, reduce: bool) {
        self.runtime.set_reduce_motion(reduce);
    }

    /// Current menu-layer screen for binding refresh.
    #[must_use]
    pub fn active_screen(&self, layer: Layer) -> Option<Screen> {
        self.active
            .iter()
            .rev()
            .find(|entry| entry.layer == layer)
            .map(|entry| entry.screen)
    }

    /// Updates a named number binding.
    pub fn set_num(&mut self, key: &str, value: f32) {
        self.bindings.set_num(key, value);
    }

    /// Updates a named flag binding.
    pub fn set_flag(&mut self, key: &str, value: bool) {
        self.bindings.set_flag(key, value);
    }

    /// Updates a named text binding.
    pub fn set_text(&mut self, key: &str, value: &str) {
        self.bindings.set_text(key, value);
    }

    /// Updates a named list binding.
    pub fn set_list(&mut self, key: &str, list: ListBinding) {
        self.bindings.set_list(key, list);
    }

    /// Binds the final actor stats and team outcome to the results document.
    pub fn set_results(&mut self, summary: &crate::results::MatchSummary, score: u32) {
        let player_team = summary.actors.first().map(|actor| actor.team);
        let placement = match (player_team, summary.winner_team) {
            (Some(team), Some(winner)) if team == winner => "VICTORY",
            (_, Some(_)) => "DEFEAT",
            _ => "DRAW",
        };
        let winner = summary
            .winner_team
            .map_or_else(|| "TIED MATCH".to_owned(), |team| format!("TEAM {team}"));
        self.bindings.set_text("placement", placement);
        self.bindings.set_text("winner", &winner);
        self.bindings
            .set_num("kills", display_number(summary.player_kills()));
        self.bindings
            .set_num("deaths", display_number(summary.player_deaths()));
        self.set_result_score(score);
    }

    /// Updates the count-up value displayed on Results.
    pub fn set_result_score(&mut self, score: u32) {
        self.bindings.set_num("score", display_number(score));
    }

    /// Number of actions emitted by the most recent menu frame.
    #[must_use]
    pub fn action_count(&self) -> usize {
        self.events.len()
    }

    /// Copies one typed action emitted by the most recent menu frame.
    #[must_use]
    pub fn action(&self, index: usize) -> Option<MenuAction> {
        self.events.get(index).copied()
    }
}
