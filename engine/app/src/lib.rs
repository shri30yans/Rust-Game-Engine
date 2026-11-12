//! Engine app: the host a game plugs into. It holds the fixed-tick clock, the game
//! registration trait, the app that owns the world, schedule, clock and job pools, the
//! headless runner, and the single-threaded client runner (window, tick, extract, blend, draw).
//! The threaded mode, the server runner and the audio and network hosts come in later slices.
//!
//! Modules:
//! - `clock`: integer nanosecond accumulator that turns frame time into ticks.
//! - `game`: the [`Game`] and [`NetGame`] traits, player identity and [`GameInfo`].
//! - `config`: [`AppConfig`] start-up settings.
//! - `settings_file`: [`SettingsFile`] loads and saves a game's versioned settings through the platform store.
//! - `app`: [`App`], the fixed-tick loop.
//! - `headless`: [`run_headless`], which records a state hash per tick.
//! - `error`: [`AppError`].
//! - `extract`: [`Extractor`] copies transforms into a pre-sized [`RenderFrame`].
//! - `exchange`: [`frame_exchange`], the two-frame hand-off between simulation and render.
//! - `late_camera`: [`ViewAngles`], [`LookDelta`], [`LookSensitivity`] and the shared [`LookInbox`].
//! - `blend`: [`blend`], the pure interpolation between the last two ticks.
//! - `client`: [`ClientApp`], [`Presenter`] and [`run_client`], the single-threaded client.
//! - `net_server`: [`NetServer`] and deterministic dedicated-server ticks.
//! - `net_client`: [`NetClient`] and client prediction/interpolation.
//! - `scene_present`: [`ScenePresenter`] draws every entity that carries a scene `Visual`.
//! - `error_banner`: [`ErrorBanner`], the red bar the client draws after the presenter's UI.
//! - `telemetry`: [`Telemetry`] and [`FrameReport`], the frame-time window and its percentiles.
//! - `overlay`: [`draw_overlay`], the telemetry panel any game's presenter can draw.
//!
//! Invariants: frame time arrives as `dt` from the platform. The one clock read is the client
//! runner's timing of its simulation step for the telemetry; it is never fed back into the
//! simulation. No OS entropy, no global mutable state, no panics or unchecked indexing in non-test
//! code. The crate never names a game, mode, map or weapon.
#![forbid(unsafe_code)]

mod app;
mod asset_uploader;
mod blend;
mod client;
mod clock;
mod config;
mod error;
mod error_banner;
mod exchange;
mod extract;
mod game;
pub mod headless;
mod late_camera;
mod live_scene;
mod menus_present;
mod net_client;
mod net_server;
mod overlay;
mod scene_present;
mod settings_file;
mod startup;
mod telemetry;
mod ui_present;

pub use app::App;
pub use asset_uploader::{AssetUploader, ClientAssets, GpuPack, UploadError, UploadStatus};
pub use blend::blend;
pub use client::{ClientApp, ClientSession, Presenter, Session, run_client};
pub use clock::{Advance, FixedClock, TickRate};
pub use config::AppConfig;
pub use engine_audio::{
    Attenuation, Audio, AudioError, AudioPlayer, Bus, Emit, SoundBank, SoundDef, SoundId, Volume,
    new_mock as new_mock_audio,
};
pub use error::AppError;
pub use error_banner::ErrorBanner;
pub use exchange::{RenderEnd, SimEnd, frame_exchange};
pub use extract::{ExtractedTransform, ExtractedVisual, Extractor, RenderFrame};
pub use game::{Game, GameInfo, NetGame, PlayerId};
pub use headless::run_headless;
pub use late_camera::{LookDelta, LookInbox, LookSensitivity, ViewAngles};
pub use live_scene::{LevelDiff, LiveScene, apply_level_diff, diff_levels};
pub use menus_present::{MenuInputMapper, draw_menus, screen_size as menu_screen_size};
pub use net_client::NetClient;
pub use net_server::{NetServer, ServerTick, run_dedicated};
pub use overlay::{Severity, TELEMETRY_TOUCH_ID, display_interval_ms, draw_overlay, severity};
pub use scene_present::{ScenePresenter, Style, draw_matrix, style_of};
pub use settings_file::{SettingsFile, SettingsFileError, SettingsWarning, VersionedSettings};
pub use startup::{GameEntry, Start, run_game};
pub use telemetry::{FrameReport, REFRESH_RATES_HZ, Telemetry, WINDOW_FRAMES};
pub use ui_present::UiScreen;

/// Declares the desktop and NativeActivity process entry for one game module.
#[macro_export]
macro_rules! game_main {
    ($entry:ty) => {
        platform::platform_game_main!($entry, |source| $crate::run_game::<$entry>(source));
    };
}
