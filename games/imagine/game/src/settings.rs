//! Persisted player preferences and bounded changes shared by IMAGINE's menus and presentation.
//!
//! Invariants: a sensitivity step is always within `SENS_MIN..=SENS_MAX` (changes saturate); the
//! sensitivity steps remain in range and graphics previews use only simulation ticks. The engine
//! saves player values to the platform config store.

use app::VersionedSettings;
use app::{AppError, LookSensitivity};
use serde::{Deserialize, Serialize};

/// Version of the persistent settings shape.
pub const SETTINGS_VERSION: u32 = 1;

/// The lowest sensitivity step.
pub const SENS_MIN: u8 = 1;
/// The highest sensitivity step.
pub const SENS_MAX: u8 = 20;
/// The step a new session starts at: 0.004 radians per pixel, the engine default (D49).
pub const SENS_DEFAULT: u8 = 10;
/// Minimum master volume percentage.
pub const VOLUME_MIN: u8 = 0;
/// Maximum master volume percentage.
pub const VOLUME_MAX: u8 = 100;
/// Default master volume percentage.
pub const VOLUME_DEFAULT: u8 = 80;
/// Minimum render scale percentage.
pub const RENDER_SCALE_MIN: u8 = 50;
/// Maximum render scale percentage.
pub const RENDER_SCALE_MAX: u8 = 100;
/// Default render scale percentage.
pub const RENDER_SCALE_DEFAULT: u8 = 100;
/// Minimum menu and match field of view.
pub const FOV_MIN: u8 = 45;
/// Maximum menu and match field of view.
pub const FOV_MAX: u8 = 75;
/// Default field of view.
pub const FOV_DEFAULT: u8 = 55;
/// A graphics preview expires after ten seconds at the game's 60 Hz tick rate.
pub const GRAPHICS_REVERT_TICKS: u16 = 600;
/// Radians of turn per pixel of drag, per sensitivity step.
const RADIANS_PER_STEP: f32 = 0.0004;

/// How the rifle is fired.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum FireMode {
    /// The rifle fires only while a fire control is held.
    Manual,
    /// The rifle also fires whenever an enemy is under the crosshair.
    Auto,
}

/// Saved graphics quality preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum QualityTier {
    /// Low preset.
    Low,
    /// Medium preset.
    #[default]
    Medium,
    /// High preset.
    High,
    /// Ultra preset.
    Ultra,
}

impl QualityTier {
    /// Stable menu index for this preset.
    pub const fn index(self) -> u16 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
            Self::Ultra => 3,
        }
    }

    /// Resolves a menu index to a supported preset.
    pub const fn from_index(index: u16) -> Option<Self> {
        match index {
            0 => Some(Self::Low),
            1 => Some(Self::Medium),
            2 => Some(Self::High),
            3 => Some(Self::Ultra),
            _ => None,
        }
    }
}

/// Settings that revert unless the player confirms a graphics change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphicsSettings {
    /// Quality preset.
    pub quality: QualityTier,
    /// Internal render scale in percent.
    pub render_scale: u8,
    /// Vertical field of view.
    pub fov: u8,
}

/// Tick-driven rollback state for an unconfirmed graphics preview.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphicsRevert {
    original: Option<GraphicsSettings>,
    ticks_left: u16,
}

impl GraphicsRevert {
    /// Begins a preview from the saved values if one is not already active.
    pub fn begin(&mut self, saved: GraphicsSettings) {
        if self.original.is_none() {
            self.original = Some(saved);
        }
        self.ticks_left = GRAPHICS_REVERT_TICKS;
    }

    /// Confirms the preview so the caller can persist the current values.
    pub fn confirm(&mut self) {
        self.original = None;
        self.ticks_left = 0;
    }

    /// Advances one fixed tick and returns the original values once the preview expires.
    pub fn tick(&mut self) -> Option<GraphicsSettings> {
        self.original?;
        self.ticks_left = self.ticks_left.saturating_sub(1);
        if self.ticks_left == 0 {
            let original = self.original.take();
            return original;
        }
        None
    }
}

impl FireMode {
    /// The label the menu shows.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Manual => "MANUAL",
            Self::Auto => "AUTO",
        }
    }
}

/// The player's persisted preferences.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Settings {
    sens_x: u8,
    sens_y: u8,
    mode: FireMode,
    #[serde(default = "default_volume")]
    volume: u8,
    #[serde(default = "default_volume")]
    music_volume: u8,
    #[serde(default = "default_volume")]
    sfx_volume: u8,
    #[serde(default = "default_volume")]
    voice_volume: u8,
    #[serde(default)]
    quality: QualityTier,
    #[serde(default = "default_render_scale")]
    render_scale: u8,
    #[serde(default = "default_fov")]
    fov: u8,
    #[serde(default)]
    reduce_motion: bool,
    #[serde(default = "default_aim_assist")]
    aim_assist: bool,
    #[serde(default)]
    fire_left: bool,
}

impl PartialEq for Settings {
    fn eq(&self, other: &Self) -> bool {
        (
            self.sens_x,
            self.sens_y,
            self.mode,
            self.volume,
            self.music_volume,
            self.sfx_volume,
            self.voice_volume,
            self.quality,
            self.render_scale,
            self.fov,
            self.reduce_motion,
        ) == (
            other.sens_x,
            other.sens_y,
            other.mode,
            other.volume,
            other.music_volume,
            other.sfx_volume,
            other.voice_volume,
            other.quality,
            other.render_scale,
            other.fov,
            other.reduce_motion,
        ) && self.aim_assist == other.aim_assist
            && self.fire_left == other.fire_left
    }
}

impl Eq for Settings {}

impl VersionedSettings for Settings {
    const VERSION: u32 = SETTINGS_VERSION;

    fn validate(&mut self) {
        self.sens_x = self.sens_x.clamp(SENS_MIN, SENS_MAX);
        self.sens_y = self.sens_y.clamp(SENS_MIN, SENS_MAX);
        self.volume = self.volume.clamp(VOLUME_MIN, VOLUME_MAX);
        self.music_volume = self.music_volume.clamp(VOLUME_MIN, VOLUME_MAX);
        self.sfx_volume = self.sfx_volume.clamp(VOLUME_MIN, VOLUME_MAX);
        self.voice_volume = self.voice_volume.clamp(VOLUME_MIN, VOLUME_MAX);
        self.render_scale = self.render_scale.clamp(RENDER_SCALE_MIN, RENDER_SCALE_MAX);
        self.fov = self.fov.clamp(FOV_MIN, FOV_MAX);
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sens_x: SENS_DEFAULT,
            sens_y: SENS_DEFAULT,
            mode: FireMode::Manual,
            volume: VOLUME_DEFAULT,
            music_volume: VOLUME_DEFAULT,
            sfx_volume: VOLUME_DEFAULT,
            voice_volume: VOLUME_DEFAULT,
            quality: QualityTier::Medium,
            render_scale: RENDER_SCALE_DEFAULT,
            fov: FOV_DEFAULT,
            reduce_motion: false,
            aim_assist: true,
            fire_left: false,
        }
    }
}

fn default_volume() -> u8 {
    VOLUME_DEFAULT
}

fn default_render_scale() -> u8 {
    RENDER_SCALE_DEFAULT
}

fn default_fov() -> u8 {
    FOV_DEFAULT
}

fn default_aim_assist() -> bool {
    true
}

fn step(value: u8, delta: i8) -> u8 {
    value.saturating_add_signed(delta).clamp(SENS_MIN, SENS_MAX)
}

impl Settings {
    /// Horizontal sensitivity step.
    pub const fn sens_x(&self) -> u8 {
        self.sens_x
    }

    /// Vertical sensitivity step.
    pub const fn sens_y(&self) -> u8 {
        self.sens_y
    }

    /// The shooting mode.
    pub const fn mode(&self) -> FireMode {
        self.mode
    }

    /// Master volume percentage.
    pub const fn volume(&self) -> u8 {
        self.volume
    }

    /// Music volume percentage.
    pub const fn music_volume(&self) -> u8 {
        self.music_volume
    }

    /// Sound effects volume percentage.
    pub const fn sfx_volume(&self) -> u8 {
        self.sfx_volume
    }

    /// Voice volume percentage.
    pub const fn voice_volume(&self) -> u8 {
        self.voice_volume
    }

    /// Saved graphics settings.
    #[must_use]
    pub const fn graphics(&self) -> GraphicsSettings {
        GraphicsSettings {
            quality: self.quality,
            render_scale: self.render_scale,
            fov: self.fov,
        }
    }

    /// Replaces graphics settings after validation by the caller.
    pub fn set_graphics(&mut self, graphics: GraphicsSettings) {
        self.quality = graphics.quality;
        self.render_scale = graphics
            .render_scale
            .clamp(RENDER_SCALE_MIN, RENDER_SCALE_MAX);
        self.fov = graphics.fov.clamp(FOV_MIN, FOV_MAX);
    }

    /// Changes the saved graphics quality preset.
    pub fn set_quality(&mut self, quality: QualityTier) {
        self.quality = quality;
    }

    /// Updates one range control and reports whether it changed the value.
    pub fn set_range(&mut self, control: super::menus::SettingControl, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let value = value.round();
        match control {
            super::menus::SettingControl::SensitivityX => {
                set_u8(&mut self.sens_x, value, SENS_MIN, SENS_MAX)
            }
            super::menus::SettingControl::SensitivityY => {
                set_u8(&mut self.sens_y, value, SENS_MIN, SENS_MAX)
            }
            super::menus::SettingControl::Volume => {
                set_u8(&mut self.volume, value, VOLUME_MIN, VOLUME_MAX)
            }
            super::menus::SettingControl::MusicVolume => {
                set_u8(&mut self.music_volume, value, VOLUME_MIN, VOLUME_MAX)
            }
            super::menus::SettingControl::SfxVolume => {
                set_u8(&mut self.sfx_volume, value, VOLUME_MIN, VOLUME_MAX)
            }
            super::menus::SettingControl::VoiceVolume => {
                set_u8(&mut self.voice_volume, value, VOLUME_MIN, VOLUME_MAX)
            }
            super::menus::SettingControl::RenderScale => set_u8(
                &mut self.render_scale,
                value,
                RENDER_SCALE_MIN,
                RENDER_SCALE_MAX,
            ),
            super::menus::SettingControl::FieldOfView => {
                set_u8(&mut self.fov, value, FOV_MIN, FOV_MAX)
            }
            super::menus::SettingControl::ReduceMotion
            | super::menus::SettingControl::AimAssist
            | super::menus::SettingControl::LeftFire => false,
        }
    }

    /// Sets the reduce-motion accessibility option.
    pub fn set_reduce_motion(&mut self, reduce: bool) {
        self.reduce_motion = reduce;
    }

    /// Whether motion is reduced.
    #[must_use]
    pub const fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    /// Whether touch aim slowdown is enabled.
    #[must_use]
    pub const fn aim_assist(&self) -> bool {
        self.aim_assist
    }

    /// Sets whether touch aim slowdown is enabled.
    pub fn set_aim_assist(&mut self, enabled: bool) {
        self.aim_assist = enabled;
    }

    /// Whether the optional left fire button is enabled.
    #[must_use]
    pub const fn fire_left(&self) -> bool {
        self.fire_left
    }

    /// Sets whether the optional left fire button is enabled.
    pub fn set_fire_left(&mut self, enabled: bool) {
        self.fire_left = enabled;
    }

    /// Changes horizontal sensitivity by `delta` steps, saturating at the ends.
    pub fn nudge_x(&mut self, delta: i8) {
        self.sens_x = step(self.sens_x, delta);
    }

    /// Changes vertical sensitivity by `delta` steps, saturating at the ends.
    pub fn nudge_y(&mut self, delta: i8) {
        self.sens_y = step(self.sens_y, delta);
    }

    /// Flips between manual and auto fire.
    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            FireMode::Manual => FireMode::Auto,
            FireMode::Auto => FireMode::Manual,
        };
    }

    /// The look speed these settings give, per axis.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::InvalidLookSensitivity`] if a step maps to a non-positive speed, which
    /// the step range rules out.
    pub fn look_sensitivity(&self) -> Result<LookSensitivity, AppError> {
        LookSensitivity::per_axis(
            f32::from(self.sens_x) * RADIANS_PER_STEP,
            f32::from(self.sens_y) * RADIANS_PER_STEP,
        )
    }
}

fn set_u8(slot: &mut u8, value: f32, min: u8, max: u8) -> bool {
    let value = value.clamp(f32::from(min), f32::from(max));
    let next = (min..=max)
        .find(|candidate| f32::from(*candidate) == value)
        .unwrap_or(min);
    if *slot == next {
        false
    } else {
        *slot = next;
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn the_default_step_is_the_engine_default_speed() {
        let s = Settings::default().look_sensitivity().unwrap();
        assert!((s.x() - 0.004).abs() < 1e-6 && (s.y() - 0.004).abs() < 1e-6);
    }

    #[test]
    fn steps_saturate_at_both_ends() {
        let mut s = Settings::default();
        s.nudge_x(i8::MAX);
        s.nudge_y(i8::MIN);
        assert_eq!((s.sens_x(), s.sens_y()), (SENS_MAX, SENS_MIN));
        s.nudge_x(1);
        s.nudge_y(-1);
        assert_eq!((s.sens_x(), s.sens_y()), (SENS_MAX, SENS_MIN));
    }

    #[test]
    fn the_axes_change_independently() {
        let mut s = Settings::default();
        s.nudge_x(3);
        let look = s.look_sensitivity().unwrap();
        assert!(look.x() > look.y());
    }

    #[test]
    fn every_step_gives_a_valid_sensitivity() {
        for step in SENS_MIN..=SENS_MAX {
            let mut s = Settings::default();
            s.nudge_x(i8::try_from(step).unwrap() - i8::try_from(SENS_DEFAULT).unwrap());
            assert!(s.look_sensitivity().is_ok());
        }
    }

    #[test]
    fn validation_clamps_saved_steps() {
        let mut settings = Settings {
            sens_x: u8::MAX,
            sens_y: 0,
            mode: FireMode::Auto,
            volume: u8::MAX,
            ..Settings::default()
        };
        settings.validate();
        assert_eq!((settings.sens_x(), settings.sens_y()), (SENS_MAX, SENS_MIN));
        assert_eq!(settings.volume(), VOLUME_MAX);
    }

    #[test]
    fn saved_settings_round_trip_and_validate() {
        let mut settings = Settings::default();
        settings.set_range(super::super::menus::SettingControl::SensitivityX, 14.0);
        settings.set_range(super::super::menus::SettingControl::Volume, 42.0);
        settings.set_range(super::super::menus::SettingControl::MusicVolume, 23.0);
        settings.set_range(super::super::menus::SettingControl::SfxVolume, 32.0);
        settings.set_range(super::super::menus::SettingControl::VoiceVolume, 41.0);
        settings.set_range(super::super::menus::SettingControl::RenderScale, 75.0);
        settings.set_range(super::super::menus::SettingControl::FieldOfView, 75.0);
        settings.set_quality(QualityTier::High);
        settings.set_reduce_motion(true);
        settings.set_aim_assist(false);
        settings.set_fire_left(true);
        let encoded = ron::to_string(&settings).expect("settings serialize");
        let mut decoded: Settings = ron::from_str(&encoded).expect("settings deserialize");
        decoded.validate();
        assert_eq!(decoded.sens_x(), 14);
        assert_eq!(decoded.volume(), 42);
        assert_eq!(decoded.music_volume(), 23);
        assert_eq!(decoded.sfx_volume(), 32);
        assert_eq!(decoded.voice_volume(), 41);
        assert_eq!(decoded.graphics().render_scale, 75);
        assert_eq!(decoded.graphics().fov, 75);
        assert_eq!(decoded.graphics().quality, QualityTier::High);
        assert!(decoded.reduce_motion());
        assert!(!decoded.aim_assist());
        assert!(decoded.fire_left());
    }

    #[test]
    fn combat_controls_default_to_touch_assist_without_left_fire() {
        let settings = Settings::default();
        assert_eq!(settings.graphics().fov, 55);
        assert!(settings.aim_assist());
        assert!(!settings.fire_left());
    }

    #[test]
    fn graphics_preview_expires_on_ticks_and_confirmation_clears_it() {
        let original = Settings::default().graphics();
        let mut revert = GraphicsRevert::default();
        revert.begin(original);
        for _ in 0..GRAPHICS_REVERT_TICKS - 1 {
            assert_eq!(revert.tick(), None);
        }
        assert_eq!(revert.tick(), Some(original));
        revert.begin(original);
        revert.confirm();
        for _ in 0..GRAPHICS_REVERT_TICKS {
            assert_eq!(revert.tick(), None);
        }
    }
}
