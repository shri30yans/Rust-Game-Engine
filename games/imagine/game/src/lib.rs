//! IMAGINE: the first playable arena (Kiln Yard) built on `kit-fps`.
//!
//! The crate is a game: it uses the engine, the FPS kit and its own code, and nothing else. The
//! simulation (`sim`) and the rules (`rules`) are headless and deterministic; `present` and `hud`
//! turn that state into pixels. Tuning lives in `data`, the arena layout in `scene`.

#![deny(unsafe_code)]

pub mod audio;
pub mod collision;
pub mod data;
pub mod error;
pub mod feedback;
pub mod flow;
pub mod hearing;
pub mod hud;
pub mod menus;
pub mod present;
pub mod results;
pub mod rules;
pub mod scene;
pub mod session;
pub mod settings;
pub mod sim;
pub mod touch;

#[cfg(test)]
mod test_support;

pub use present::{ArenaPresenter, PresenterError};
pub use sim::{ImagineGame, app_config, set_player_command};

use app::{AppError, ClientApp, GameEntry, SettingsFile, Start};
use engine_assets::ContentName;
use platform::ConfigName;
use render::{GovernorConfig, Settings};

/// IMAGINE's engine-owned game module registration.
pub struct ImagineEntry;

impl GameEntry for ImagineEntry {
    type Game = ImagineGame;
    type Presenter = ArenaPresenter;

    const TITLE: &'static str = "IMAGINE";
    const CONTENT: ContentName = ContentName::from_static("imagine");

    fn build(start: &Start<'_>) -> Result<ClientApp<Self::Game, Self::Presenter>, AppError> {
        let config = app_config(Some(start.device.job_workers()))?;
        let client = ClientApp::new(
            ImagineGame::from_content(start.content),
            &crate::sim::ImagineSettings::menu_backdrop(),
            ArenaPresenter::with_settings_file(
                start.content,
                start.device.touch,
                SettingsFile::new(start.config.clone(), ConfigName::new("settings.ron")?),
            )?,
            &config,
            start.device,
            Settings::default(),
        )?;
        if start.device.class == platform::DeviceClass::Handheld {
            client.with_governor(GovernorConfig::new(16.6, 0.7, 1.0)?)
        } else {
            Ok(client)
        }
    }
}

app::game_main!(ImagineEntry);
