//! Logical pack paths for IMAGINE's authored menu documents.

use app::AppError;
use engine_assets::ContentPack;
use engine_ui::{Document, UiLayout};

use crate::flow::Screen;

/// Every menu screen loaded from the mounted game content pack.
pub(super) const MENU_SCREENS: &[Screen] = &[
    Screen::Boot,
    Screen::Home,
    Screen::Modes,
    Screen::Lobby,
    Screen::Loading,
    Screen::MatchMenu,
    Screen::Scoreboard,
    Screen::Results,
    Screen::Settings,
    Screen::Loadout,
    Screen::LeaveConfirm,
];

/// Logical content-pack path for one menu document.
pub(super) const fn document_path(screen: Screen) -> &'static str {
    match screen {
        Screen::Boot => "ui/boot.ui.ron",
        Screen::Home => "ui/home.ui.ron",
        Screen::Modes => "ui/modes.ui.ron",
        Screen::Lobby => "ui/lobby.ui.ron",
        Screen::Loading => "ui/loading.ui.ron",
        Screen::MatchMenu => "ui/match_menu.ui.ron",
        Screen::Scoreboard => "ui/scoreboard.ui.ron",
        Screen::Results => "ui/results.ui.ron",
        Screen::Settings => "ui/settings.ui.ron",
        Screen::Loadout => "ui/loadout.ui.ron",
        Screen::LeaveConfirm => "ui/confirm.ui.ron",
    }
}

pub(super) fn document_source(content: &ContentPack, screen: Screen) -> Result<String, AppError> {
    let path = engine_assets::AssetPath::new(document_path(screen))?;
    Ok(content.load_text(&path)?.to_owned())
}

impl super::runtime::GameMenus {
    pub(super) fn document_source(&self, screen: Screen) -> &str {
        self.document_overrides
            .get(&screen)
            .map_or("", String::as_str)
    }

    pub(super) fn replace_document_source(
        &mut self,
        screen: Screen,
        text: String,
    ) -> Result<(), AppError> {
        let document = Document::from_ron(&text)?;
        UiLayout::new(document)?;
        self.document_overrides.insert(screen, text);
        let version = self.document_version(screen).saturating_add(1);
        self.document_versions.insert(screen, version);
        Ok(())
    }

    pub(super) fn sync_active_documents(&mut self) -> Result<(), AppError> {
        for index in 0..self.active.len() {
            let Some(entry) = self.active.get(index) else {
                continue;
            };
            if self.document_version(entry.screen) == entry.document_version
                || self
                    .active
                    .iter()
                    .rposition(|candidate| candidate.layer == entry.layer)
                    != Some(index)
            {
                continue;
            }
            let screen = entry.screen;
            let layer = entry.layer;
            let document = Document::from_ron(self.document_source(screen))?;
            UiLayout::new(document.clone())?;
            let id = self.runtime.replace(layer, document)?;
            let version = self.document_version(screen);
            if let Some(active) = self.active.get_mut(index) {
                active.id = id;
                active.document_version = version;
            }
        }
        Ok(())
    }
}
