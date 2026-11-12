//! Engine-only walk, jump and collide demo used to prove the platform stands without a kit.
#![deny(unsafe_code)]

use app::{AppError, ClientApp, GameEntry, Start};
use engine_assets::ContentName;
use render::{GovernorConfig, Settings};

pub mod present;
pub mod scene;
pub mod sim;

#[cfg(test)]
mod test_support;

pub use present::SandboxPresenter;
pub use sim::{
    Axis, PlayerCommand, SandboxGame, app_config, place_player, player_body, player_state,
    player_yaw, set_command,
};

/// Sandbox's engine-owned game module registration.
pub struct SandboxEntry;

impl GameEntry for SandboxEntry {
    type Game = SandboxGame;
    type Presenter = SandboxPresenter;

    const TITLE: &'static str = "Sandbox";
    const CONTENT: ContentName = ContentName::from_static("sandbox");

    fn build(start: &Start<'_>) -> Result<ClientApp<Self::Game, Self::Presenter>, AppError> {
        let config = app_config(Some(start.device.job_workers()))?;
        let client = ClientApp::new(
            SandboxGame::from_content(start.content),
            &(),
            SandboxPresenter::new(start.device.touch),
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

app::game_main!(SandboxEntry);
