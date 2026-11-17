//! IMAGINE tuning data and its checked conversions to `kit-fps` configuration types.
//!
//! Designer values are loaded from `data/tuning.ron`; geometry that must match the FPS kit's
//! capsule and eye placement stays here. `Tuning::validate` checks relationships before a value
//! enters the world, and conversions build the kit's validated configs from that value.

use engine_assets::{AssetPath, ContentPack};
use engine_core::StateHasher;
use engine_scene::Validate;
use engine_world::{Resource, WorldError};
use kit_fps::{
    AimAssistParams, BotConfig, BotParams, ControllerConfig, ControllerTuning, KitError,
    LinkParams, LinkRules, WeaponConfig, WeaponParams,
};
use serde::{Deserialize, Serialize};

/// Simulation rate, in ticks per second.
pub const TICK_RATE_HZ: u16 = 60;
/// Half height of the character capsule's straight section, in metres.
pub const CAPSULE_HALF_HEIGHT_M: f32 = 0.6;
/// Radius of the character capsule, in metres.
pub const CAPSULE_RADIUS_M: f32 = 0.3;
/// Distance from an actor's centre down to its feet, in metres.
pub const CENTRE_ABOVE_FEET_M: f32 = CAPSULE_HALF_HEIGHT_M + CAPSULE_RADIUS_M;
/// Eye height above the actor's centre, in metres.
pub const EYE_ABOVE_CENTRE_M: f32 = 0.7;

/// Player-count bounds for one game mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeCapacity {
    /// Fewest players allowed in a match.
    pub min_players: u8,
    /// Most players allowed in a match.
    pub max_players: u8,
}

impl ModeCapacity {
    /// Validates the inclusive player range.
    pub fn validate(self) -> Result<(), String> {
        if self.min_players == 0 || self.min_players > self.max_players {
            return Err("mode capacity must satisfy 1 <= min_players <= max_players".to_owned());
        }
        Ok(())
    }

    /// Maximum number of bots when the host is the only human.
    #[must_use]
    pub const fn max_bots(self) -> u8 {
        self.max_players.saturating_sub(1)
    }
}

/// A data-backed mode card and its validated capacity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeDefinition {
    /// Stable mode id used by the client.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Player capacity.
    pub capacity: ModeCapacity,
    /// Duration label.
    pub length: String,
    /// One-line rule description.
    pub rule: String,
    /// Whether this build can start the mode.
    pub available: bool,
}

impl ModeDefinition {
    /// Formats the player range from the canonical capacity values.
    #[must_use]
    pub fn players_label(&self) -> String {
        if self.capacity.min_players == self.capacity.max_players {
            format!("{} PLAYER", self.capacity.max_players)
        } else {
            format!(
                "{}–{} PLAYERS",
                self.capacity.min_players, self.capacity.max_players
            )
        }
    }
}

/// Parses and validates the game's canonical mode data.
///
/// # Errors
///
/// Returns an error for malformed modes, invalid capacities or duplicate ids.
pub fn mode_definitions(source: &str) -> Result<Vec<ModeDefinition>, String> {
    let modes: Vec<ModeDefinition> = ron::from_str(source).map_err(|error| error.to_string())?;
    let mut ids = std::collections::HashSet::new();
    for mode in &modes {
        if mode.id.trim().is_empty() || mode.name.trim().is_empty() {
            return Err("mode id and name must not be empty".to_owned());
        }
        mode.capacity
            .validate()
            .map_err(|error| format!("{}: {error}", mode.name))?;
        if !ids.insert(mode.id.as_str()) {
            return Err(format!("duplicate mode id {}", mode.id));
        }
    }
    Ok(modes)
}

/// Projected fraction of screen height occupied by a target at a distance, under vertical FOV.
#[must_use]
pub fn target_screen_fraction(height_m: f32, distance_m: f32, vfov_rad: f32) -> f32 {
    height_m / (2.0 * distance_m * (vfov_rad * 0.5).tan())
}

/// IMAGINE's validated designer tuning. All durations are milliseconds and angles are degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tuning {
    /// Camera field of view in degrees. Hor+ projection keeps this vertical value fixed.
    pub camera_fov_deg: f32,
    /// Touch aim slowdown parameters.
    pub aim_assist: AimAssistTuning,
    /// Player and bot identities and hit points.
    pub actors: ActorTuning,
    /// Weapon identity and firing values.
    pub rifle: RifleTuning,
    /// Shared player and bot movement values.
    pub movement: MovementTuning,
    /// Bot perception, aim and movement values.
    pub bots: BotTuning,
    /// Bot waypoint-link values.
    pub navigation: NavigationTuning,
    /// Respawn delays.
    pub respawn: RespawnTuning,
    /// On-screen feedback lifetimes.
    pub feedback: FeedbackTuning,
    /// Rules data not owned by the FPS kit.
    pub rules: RuleTuning,
}

/// Aim slowdown tuning used to build the FPS kit's validated parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AimAssistTuning {
    /// Outer recovery ramp angle, in degrees.
    pub cone_deg: f32,
    /// Look scale while the aim ray intersects an enemy.
    pub slowdown: f32,
    /// Maximum target distance, in metres.
    pub max_range_m: f32,
}

/// Player and bot names and health values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorTuning {
    /// Player display name.
    pub player_name: String,
    /// Unique bot display names in deterministic slot order.
    pub bot_roster: Vec<String>,
    /// Player hit points.
    pub player_health: u32,
    /// Bot hit points.
    pub bot_health: u32,
}

/// Rifle values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RifleTuning {
    /// HUD display name.
    pub name: String,
    /// Damage per hit.
    pub damage: u32,
    /// Rounds per minute.
    pub rpm: u32,
    /// Rounds in a full magazine.
    pub magazine: u32,
    /// Reload duration.
    pub reload_ms: u32,
    /// Spread at rest, in degrees.
    pub spread_deg: f32,
    /// Spread added by each shot, in degrees.
    pub spread_per_shot_deg: f32,
    /// Spread recovered each second, in degrees.
    pub spread_recovery_deg_per_s: f32,
    /// Maximum shot range, in metres.
    pub range_m: f32,
    /// Damage remains full through this distance, then falls toward `min_damage_scale`.
    pub full_damage_until_m: f32,
    /// Distance where minimum damage is reached.
    pub min_damage_from_m: f32,
    /// Damage multiplier at and beyond `min_damage_from_m`.
    pub min_damage_scale: f32,
}

/// Shared movement values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementTuning {
    /// Ground speed in metres per second.
    pub walk_speed: f32,
    /// Ground acceleration in metres per second squared.
    pub ground_accel: f32,
    /// Ground friction in metres per second squared.
    pub ground_friction: f32,
    /// Air acceleration in metres per second squared.
    pub air_accel: f32,
    /// Air speed cap in metres per second.
    pub air_speed_cap: f32,
    /// Jump launch speed in metres per second.
    pub jump_speed: f32,
    /// Gravity in metres per second squared.
    pub gravity: f32,
    /// Maximum fall speed in metres per second.
    pub max_fall_speed: f32,
    /// Coyote time in ticks.
    pub coyote_ticks: u32,
    /// Jump buffer in ticks.
    pub jump_buffer_ticks: u32,
    /// Jump cut multiplier.
    pub jump_cut: f32,
    /// Step height in metres.
    pub step_height: f32,
    /// Maximum walkable slope in degrees.
    pub slope_limit_deg: f32,
    /// Ground sprint speed in metres per second.
    pub sprint_speed: f32,
    /// Crouched speed in metres per second.
    pub crouch_speed: f32,
    /// Total crouched capsule height in metres.
    pub crouch_height: f32,
    /// Total standing capsule height in metres.
    pub stand_height: f32,
    /// Minimum horizontal speed to start a slide.
    pub slide_min_speed: f32,
    /// One-time slide speed boost in metres per second.
    pub slide_boost: f32,
    /// Slide deceleration in metres per second squared.
    pub slide_friction: f32,
    /// Slide duration in milliseconds.
    pub slide_ms: u32,
    /// Mantle reach in metres.
    pub mantle_reach_m: f32,
    /// Minimum mantle height in metres.
    pub mantle_min_height_m: f32,
    /// Maximum mantle height in metres.
    pub mantle_max_height_m: f32,
    /// Mantle duration in milliseconds.
    pub mantle_ms: u32,
    /// Camera eye-height blend duration in milliseconds.
    pub camera_crouch_ms: u32,
}

/// Bot behaviour values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotTuning {
    /// Reaction time in milliseconds.
    pub reaction_ms: u32,
    /// Starting aim error in degrees.
    pub aim_error_deg: f32,
    /// Aim-error recovery in degrees per second.
    pub aim_speed_deg_per_s: f32,
    /// Sight range in metres.
    pub sight_m: f32,
    /// Field of view in degrees.
    pub fov_deg: f32,
    /// Engage range in metres.
    pub engage_m: f32,
    /// Stop distance in metres.
    pub stop_distance_m: f32,
    /// Mean strafe interval in milliseconds.
    pub strafe_period_ms: u32,
    /// Jump chance per tick.
    pub jump_chance: f32,
    /// Turn speed in degrees per second.
    pub turn_speed_deg_per_s: f32,
    /// Target forget duration in milliseconds.
    pub forget_ms: u32,
    /// Waypoint arrival radius in metres.
    pub arrive_m: f32,
    /// Stuck detection duration in milliseconds.
    pub stuck_ms: u32,
    /// Minimum progress within the stuck duration, in metres.
    pub stuck_progress_m: f32,
    /// Hearing radius in metres.
    pub hearing_m: f32,
}

/// Waypoint graph construction values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationTuning {
    /// Lower sight ray height above a waypoint, in metres.
    pub knee_height_m: f32,
    /// Upper sight ray height above a waypoint, in metres.
    pub head_height_m: f32,
    /// Longest possible waypoint link, in metres.
    pub max_link_m: f32,
}

/// Respawn delays.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RespawnTuning {
    /// Player respawn delay in milliseconds.
    pub player_ms: u32,
    /// Bot respawn delay in milliseconds.
    pub bot_ms: u32,
}

/// Feedback lifetimes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackTuning {
    /// Tracer lifetime in ticks.
    pub tracer_ticks: u32,
    /// Muzzle flash lifetime in ticks.
    pub muzzle_flash_ticks: u32,
    /// Hit marker lifetime in ticks.
    pub hit_marker_ticks: u32,
    /// Kill flash lifetime in ticks.
    pub kill_flash_ticks: u32,
    /// Damage indicator lifetime in ticks.
    pub damage_indicator_ticks: u32,
}

/// Match and HUD rules values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleTuning {
    /// Kill-feed lifetime in milliseconds.
    pub kill_feed_ms: u32,
}

impl Tuning {
    /// Validates the kit configurations and cross-field data relationships.
    ///
    /// # Errors
    ///
    /// Returns a string naming the first invalid field path.
    pub fn validate_fields(&self) -> Result<(), String> {
        if !self.camera_fov_deg.is_finite() || !(45.0..=75.0).contains(&self.camera_fov_deg) {
            return Err("camera_fov_deg must be between 45 and 75".into());
        }
        self.aim_assist_params()
            .map_err(|error| format!("aim_assist: {error}"))?;
        if self.actors.player_name.trim().is_empty() {
            return Err("actors.player_name must not be empty".into());
        }
        let mut names = std::collections::HashSet::new();
        names.insert(self.actors.player_name.as_str());
        for (index, name) in self.actors.bot_roster.iter().enumerate() {
            if name.trim().is_empty() {
                return Err(format!("actors.bot_roster[{index}] must not be empty"));
            }
            if !names.insert(name.as_str()) {
                return Err(format!(
                    "actors.bot_roster[{index}] duplicates an actor name"
                ));
            }
        }
        if self.rifle.name.trim().is_empty() {
            return Err("rifle.name must not be empty".into());
        }
        if self.actors.player_health == 0 {
            return Err("actors.player_health must be positive".into());
        }
        if self.actors.bot_health == 0 {
            return Err("actors.bot_health must be positive".into());
        }
        if self.movement.crouch_height <= CAPSULE_RADIUS_M * 2.0 {
            return Err("movement.crouch_height must exceed twice the capsule radius".into());
        }
        if self.movement.stand_height <= CAPSULE_RADIUS_M * 2.0 {
            return Err("movement.stand_height must exceed twice the capsule radius".into());
        }
        if self.rifle.rpm == 0 {
            return Err("rifle.rpm must be positive".into());
        }
        if self.rifle.reload_ms == 0 {
            return Err("rifle.reload_ms must be positive".into());
        }
        if self.bots.hearing_m <= 0.0 || !self.bots.hearing_m.is_finite() {
            return Err("bots.hearing_m must be positive and finite".into());
        }
        self.controller_config()
            .map_err(|error| format!("movement: {error}"))?;
        self.weapon_config()
            .map_err(|error| format!("rifle: {error}"))?;
        self.bot_config()
            .map_err(|error| format!("bots: {error}"))?;
        self.link_rules()
            .map_err(|error| format!("navigation: {error}"))?;
        for (path, ms) in [
            ("rifle.reload_ms", self.rifle.reload_ms),
            ("movement.slide_ms", self.movement.slide_ms),
            ("movement.mantle_ms", self.movement.mantle_ms),
            ("movement.camera_crouch_ms", self.movement.camera_crouch_ms),
            ("bots.reaction_ms", self.bots.reaction_ms),
            ("bots.strafe_period_ms", self.bots.strafe_period_ms),
            ("bots.forget_ms", self.bots.forget_ms),
            ("bots.stuck_ms", self.bots.stuck_ms),
            ("respawn.player_ms", self.respawn.player_ms),
            ("respawn.bot_ms", self.respawn.bot_ms),
            ("rules.kill_feed_ms", self.rules.kill_feed_ms),
        ] {
            if ms_to_ticks(ms).is_none_or(|ticks| ticks == 0) {
                return Err(format!("{path} must produce at least one tick"));
            }
        }
        for (path, ticks) in [
            ("feedback.tracer_ticks", self.feedback.tracer_ticks),
            (
                "feedback.muzzle_flash_ticks",
                self.feedback.muzzle_flash_ticks,
            ),
            ("feedback.hit_marker_ticks", self.feedback.hit_marker_ticks),
            ("feedback.kill_flash_ticks", self.feedback.kill_flash_ticks),
            (
                "feedback.damage_indicator_ticks",
                self.feedback.damage_indicator_ticks,
            ),
        ] {
            if ticks == 0 {
                return Err(format!("{path} must be positive"));
            }
        }
        Ok(())
    }

    fn validate_mode_roster(&self, modes: &[ModeDefinition]) -> Result<(), String> {
        for mode in modes {
            if mode.available
                && self.actors.bot_roster.len() < usize::from(mode.capacity.max_bots())
            {
                return Err(format!(
                    "mode {} needs {} unique bot names, roster has {}",
                    mode.name,
                    mode.capacity.max_bots(),
                    self.actors.bot_roster.len()
                ));
            }
        }
        Ok(())
    }

    /// Builds validated aim slowdown parameters from designer tuning.
    ///
    /// # Errors
    ///
    /// Returns a world error with the kit's typed validation failure as its source.
    pub fn aim_assist_params(&self) -> Result<AimAssistParams, WorldError> {
        kit_fps::error::lift(
            AimAssistParams::new(
                self.aim_assist.cone_deg,
                self.aim_assist.slowdown,
                self.aim_assist.max_range_m,
            )
            .map_err(KitError::from),
        )
    }

    /// Builds the FPS controller config from this tuning.
    ///
    /// # Errors
    ///
    /// Returns the kit's validation error.
    pub fn controller_config(&self) -> Result<ControllerConfig, WorldError> {
        let m = self.movement;
        kit_fps::error::lift(
            ControllerConfig::new(ControllerTuning {
                walk_speed: m.walk_speed,
                ground_accel: m.ground_accel,
                ground_friction: m.ground_friction,
                air_accel: m.air_accel,
                air_speed_cap: m.air_speed_cap,
                jump_speed: m.jump_speed,
                gravity: m.gravity,
                max_fall_speed: m.max_fall_speed,
                coyote_ticks: m.coyote_ticks,
                jump_buffer_ticks: m.jump_buffer_ticks,
                jump_cut: m.jump_cut,
                step_height: m.step_height,
                slope_limit: m.slope_limit_deg.to_radians(),
                sprint_speed: m.sprint_speed,
                crouch_speed: m.crouch_speed,
                stand_height: m.stand_height,
                crouch_height: m.crouch_height,
                slide_min_speed: m.slide_min_speed,
                slide_boost: m.slide_boost,
                slide_friction: m.slide_friction,
                slide_ticks: movement_ticks(m.slide_ms)?,
                mantle_reach: m.mantle_reach_m,
                mantle_min_height: m.mantle_min_height_m,
                mantle_max_height: m.mantle_max_height_m,
                mantle_ticks: movement_ticks(m.mantle_ms)?,
                camera_crouch_ticks: movement_ticks(m.camera_crouch_ms)?,
            })
            .map(|config| config.query_filter(crate::collision::character_query())),
        )
    }

    /// Builds the rifle config from this tuning.
    ///
    /// # Errors
    ///
    /// Returns a world error when duration conversion overflows or the kit rejects a value.
    pub fn weapon_config(&self) -> Result<WeaponConfig, WorldError> {
        let interval = u32::from(TICK_RATE_HZ)
            .checked_mul(60)
            .and_then(|ticks| ticks.checked_div(self.rifle.rpm))
            .filter(|ticks| *ticks > 0)
            .ok_or(WorldError::Invariant(
                "rifle.rpm gives an invalid fire interval",
            ))?;
        let reload = ms_to_ticks(self.rifle.reload_ms)
            .ok_or(WorldError::Invariant("rifle.reload_ms overflows ticks"))?;
        kit_fps::error::lift(
            WeaponConfig::new(WeaponParams {
                damage: self.rifle.damage,
                fire_interval_ticks: interval,
                magazine: self.rifle.magazine,
                reload_ticks: reload,
                spread_rad: self.rifle.spread_deg.to_radians(),
                spread_per_shot: self.rifle.spread_per_shot_deg.to_radians(),
                spread_recovery_per_tick: self.rifle.spread_recovery_deg_per_s.to_radians()
                    / f32::from(TICK_RATE_HZ),
                max_range: self.rifle.range_m,
                damage_falloff: kit_fps::DamageFalloff {
                    full_until_m: self.rifle.full_damage_until_m,
                    min_from_m: self.rifle.min_damage_from_m,
                    min_scale: self.rifle.min_damage_scale,
                },
                pellets: 1,
                friendly_fire: false,
            })
            .map(|config| config.query_filter(crate::collision::projectile_query())),
        )
    }

    /// Builds the bot config from this tuning.
    ///
    /// # Errors
    ///
    /// Returns a world error when duration conversion overflows or the kit rejects a value.
    pub fn bot_config(&self) -> Result<BotConfig, WorldError> {
        let b = self.bots;
        let ticks =
            |ms| ms_to_ticks(ms).ok_or(WorldError::Invariant("bot duration overflows ticks"));
        kit_fps::error::lift(BotConfig::new(BotParams {
            sight_range: b.sight_m,
            fov_rad: b.fov_deg.to_radians(),
            reaction_ticks: ticks(b.reaction_ms)?,
            aim_speed_rad_per_s: b.aim_speed_deg_per_s.to_radians(),
            aim_error_rad: b.aim_error_deg.to_radians(),
            engage_range: b.engage_m,
            stop_distance: b.stop_distance_m,
            strafe_period_ticks: ticks(b.strafe_period_ms)?,
            jump_chance: b.jump_chance,
            turn_speed: b.turn_speed_deg_per_s.to_radians(),
            forget_ticks: ticks(b.forget_ms)?,
            arrive_radius: b.arrive_m,
            stuck_ticks: ticks(b.stuck_ms)?,
            stuck_progress: b.stuck_progress_m,
        }))
    }

    /// Builds the waypoint link rules from this tuning.
    ///
    /// # Errors
    ///
    /// Returns a world error when the kit rejects a value.
    pub fn link_rules(&self) -> Result<LinkRules, WorldError> {
        kit_fps::error::lift(LinkRules::new(LinkParams {
            clearance_radius: CAPSULE_RADIUS_M,
            knee_height: self.navigation.knee_height_m,
            head_height: self.navigation.head_height_m,
            max_step: self.movement.step_height,
            max_link: self.navigation.max_link_m,
        }))
    }
}

impl Validate for Tuning {
    fn validate(&self) -> Result<(), String> {
        self.validate_fields()
    }
}

/// The logical pack entry holding the game's mode definitions.
const MODES_PATH: &str = "data/modes.ron";

/// Parses and validates the mode definitions from their logical pack entry.
pub fn load_modes(content: &ContentPack) -> Result<Vec<ModeDefinition>, engine_scene::SceneError> {
    let modes_path =
        AssetPath::new(MODES_PATH).map_err(|error| engine_scene::SceneError::Tuning {
            file: MODES_PATH.to_owned(),
            reason: error.to_string(),
        })?;
    mode_definitions(content.load_text(&modes_path).map_err(|error| {
        engine_scene::SceneError::Tuning {
            file: MODES_PATH.to_owned(),
            reason: error.to_string(),
        }
    })?)
    .map_err(|reason| engine_scene::SceneError::Tuning {
        file: MODES_PATH.to_owned(),
        reason,
    })
}

/// Parses and validates tuning from its logical pack entry.
pub fn load_tuning(content: &ContentPack) -> Result<Tuning, engine_scene::SceneError> {
    const PATH: &str = "data/tuning.ron";
    let path = AssetPath::new(PATH).map_err(|error| engine_scene::SceneError::Tuning {
        file: PATH.to_owned(),
        reason: error.to_string(),
    })?;
    let text = content
        .load_text(&path)
        .map_err(|error| engine_scene::SceneError::Tuning {
            file: PATH.to_owned(),
            reason: error.to_string(),
        })?;
    let tuning: Tuning = ron::from_str(text).map_err(|error| engine_scene::SceneError::Tuning {
        file: PATH.to_owned(),
        reason: error.to_string(),
    })?;
    tuning
        .validate_fields()
        .map_err(|reason| engine_scene::SceneError::Tuning {
            file: PATH.to_owned(),
            reason,
        })?;
    let modes = load_modes(content)?;
    tuning
        .validate_mode_roster(&modes)
        .map_err(|reason| engine_scene::SceneError::Tuning {
            file: MODES_PATH.to_owned(),
            reason,
        })?;
    Ok(tuning)
}

impl Resource for Tuning {
    fn hash_into(&self, hasher: &mut StateHasher) {
        hasher.write_u64(u64::try_from(self.actors.bot_roster.len()).unwrap_or(u64::MAX));
        hasher.write_bytes(&self.actors.player_health.to_le_bytes());
        hasher.write_bytes(&self.actors.bot_health.to_le_bytes());
        hasher.write_bytes(&self.rifle.damage.to_le_bytes());
        hasher.write_bytes(&self.rifle.rpm.to_le_bytes());
        hasher.write_bytes(&self.rifle.magazine.to_le_bytes());
        for name in [&self.rifle.name, &self.actors.player_name] {
            hasher.write_u64(u64::try_from(name.len()).unwrap_or(u64::MAX));
            hasher.write_bytes(name.as_bytes());
        }
        for name in &self.actors.bot_roster {
            hasher.write_u64(u64::try_from(name.len()).unwrap_or(u64::MAX));
            hasher.write_bytes(name.as_bytes());
        }
        let f = [
            self.camera_fov_deg,
            self.aim_assist.cone_deg,
            self.aim_assist.slowdown,
            self.aim_assist.max_range_m,
            self.rifle.spread_deg,
            self.rifle.spread_per_shot_deg,
            self.rifle.spread_recovery_deg_per_s,
            self.rifle.range_m,
            self.rifle.full_damage_until_m,
            self.rifle.min_damage_from_m,
            self.rifle.min_damage_scale,
            self.movement.walk_speed,
            self.movement.ground_accel,
            self.movement.ground_friction,
            self.movement.air_accel,
            self.movement.air_speed_cap,
            self.movement.jump_speed,
            self.movement.gravity,
            self.movement.max_fall_speed,
            self.movement.jump_cut,
            self.movement.step_height,
            self.movement.slope_limit_deg,
            self.movement.sprint_speed,
            self.movement.crouch_speed,
            self.movement.crouch_height,
            self.movement.stand_height,
            self.movement.slide_min_speed,
            self.movement.slide_boost,
            self.movement.slide_friction,
            self.movement.mantle_reach_m,
            self.movement.mantle_min_height_m,
            self.movement.mantle_max_height_m,
            self.bots.aim_error_deg,
            self.bots.aim_speed_deg_per_s,
            self.bots.sight_m,
            self.bots.fov_deg,
            self.bots.engage_m,
            self.bots.stop_distance_m,
            self.bots.jump_chance,
            self.bots.turn_speed_deg_per_s,
            self.bots.arrive_m,
            self.bots.stuck_progress_m,
            self.bots.hearing_m,
            self.navigation.knee_height_m,
            self.navigation.head_height_m,
            self.navigation.max_link_m,
        ];
        for value in f {
            hasher.write_f32(value);
        }
        for value in [
            self.actors.player_health,
            self.actors.bot_health,
            self.rifle.reload_ms,
            self.movement.coyote_ticks,
            self.movement.jump_buffer_ticks,
            self.movement.slide_ms,
            self.movement.mantle_ms,
            self.movement.camera_crouch_ms,
            self.bots.reaction_ms,
            self.bots.strafe_period_ms,
            self.bots.forget_ms,
            self.bots.stuck_ms,
            self.respawn.player_ms,
            self.respawn.bot_ms,
            self.feedback.tracer_ticks,
            self.feedback.muzzle_flash_ticks,
            self.feedback.hit_marker_ticks,
            self.feedback.kill_flash_ticks,
            self.feedback.damage_indicator_ticks,
            self.rules.kill_feed_ms,
        ] {
            hasher.write_u32(value);
        }
    }
}

/// Converts whole milliseconds to ticks, rounded down.
#[must_use]
pub fn ms_to_ticks(ms: u32) -> Option<u32> {
    ms.checked_mul(u32::from(TICK_RATE_HZ))?.checked_div(1_000)
}

fn movement_ticks(milliseconds: u32) -> Result<u16, WorldError> {
    let ticks =
        ms_to_ticks(milliseconds)
            .filter(|ticks| *ticks > 0)
            .ok_or(WorldError::Invariant(
                "movement duration must produce at least one tick",
            ))?;
    u16::try_from(ticks).map_err(|_| WorldError::Invariant("movement duration exceeds u16 ticks"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn embedded() -> Result<Tuning, engine_scene::SceneError> {
        load_tuning(&crate::test_support::pack().expect("test pack"))
    }

    #[test]
    fn shipped_tuning_parses_and_validates() {
        let tuning = embedded().expect("embedded tuning");
        assert_eq!(tuning.actors.bot_roster.len(), 6);
        assert!(tuning.weapon_config().is_ok());
    }

    #[test]
    fn default_camera_fov_meets_target_size_at_fifteen_and_twelve_metres() {
        let tuning = embedded().expect("embedded tuning");
        assert_eq!(
            f32::from(crate::settings::Settings::default().graphics().fov),
            tuning.camera_fov_deg,
            "the player setting starts at the data-defined camera FOV"
        );
        let vfov = tuning.camera_fov_deg.to_radians();
        let at_fifteen = target_screen_fraction(tuning.movement.stand_height, 15.0, vfov);
        let at_twelve = target_screen_fraction(tuning.movement.stand_height, 12.0, vfov);
        assert!(
            at_fifteen >= 0.11,
            "15 m target covers {:.1}%",
            at_fifteen * 100.0
        );
        assert!(
            at_twelve >= 0.14,
            "12 m target covers {:.1}%",
            at_twelve * 100.0
        );
    }

    #[test]
    fn tuning_round_trips_through_ron() {
        let tuning = embedded().expect("embedded tuning");
        let text = ron::to_string(&tuning).expect("RON encodes");
        let decoded: Tuning = ron::from_str(&text).expect("RON decodes");
        decoded.validate().expect("round-tripped tuning validates");
        assert_eq!(decoded, tuning);
    }

    #[test]
    fn invalid_values_report_their_group_path() {
        let content = crate::test_support::pack().expect("test pack");
        let mut tuning = load_tuning(&content).expect("embedded tuning");
        tuning.actors.bot_roster.pop();
        let modes_path = AssetPath::new("data/modes.ron").expect("mode path");
        let modes = mode_definitions(content.load_text(&modes_path).expect("mode data"))
            .expect("valid modes");
        let error = tuning
            .validate_mode_roster(&modes)
            .expect_err("short bot roster rejected");
        assert!(error.contains("PRACTICE"), "expected mode in `{error}`");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.camera_fov_deg = 44.0;
        assert_path(tuning, "camera_fov_deg");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.camera_fov_deg = 76.0;
        assert_path(tuning, "camera_fov_deg");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.camera_fov_deg = f32::NAN;
        assert_path(tuning, "camera_fov_deg");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.aim_assist.cone_deg = 0.0;
        assert_path(tuning, "aim_assist");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.aim_assist.slowdown = 0.0;
        assert_path(tuning, "aim_assist");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.aim_assist.max_range_m = f32::INFINITY;
        assert_path(tuning, "aim_assist");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.rifle.damage = 0;
        assert_path(tuning, "rifle");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.movement.slope_limit_deg = 90.0;
        assert_path(tuning, "movement");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.bots.engage_m = tuning.bots.sight_m + 1.0;
        assert_path(tuning, "bots");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.navigation.max_link_m = 0.0;
        assert_path(tuning, "navigation");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.respawn.player_ms = 0;
        assert_path(tuning, "respawn.player_ms");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.feedback.tracer_ticks = 0;
        assert_path(tuning, "feedback.tracer_ticks");
        let mut tuning = embedded().expect("embedded tuning");
        tuning.rules.kill_feed_ms = 0;
        assert_path(tuning, "rules.kill_feed_ms");
    }

    #[test]
    fn mode_capacity_rejects_zero_and_reversed_ranges() {
        assert!(
            ModeCapacity {
                min_players: 0,
                max_players: 7,
            }
            .validate()
            .is_err()
        );
        assert!(
            ModeCapacity {
                min_players: 8,
                max_players: 7,
            }
            .validate()
            .is_err()
        );
        assert!(
            ModeCapacity {
                min_players: 1,
                max_players: 7,
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn player_label_is_derived_from_mode_capacity() {
        let content = crate::test_support::pack().expect("test pack");
        let modes_path = AssetPath::new("data/modes.ron").expect("mode path");
        let mode = mode_definitions(content.load_text(&modes_path).expect("mode data"))
            .expect("valid modes")
            .into_iter()
            .find(|mode| mode.id == "practice")
            .expect("practice mode");
        assert_eq!(mode.players_label(), "1–7 PLAYERS");
    }

    fn assert_path(tuning: Tuning, path: &str) {
        let error = tuning
            .validate_fields()
            .expect_err("invalid value rejected");
        assert!(error.contains(path), "expected `{path}` in `{error}`");
    }

    proptest! {
        #[test]
        fn every_generated_valid_tuning_converts_to_kit_configs(
            walk_speed in 0.1f32..30.0,
            damage in 1u32..1000,
            sight in 5.0f32..100.0,
            engage_fraction in 0.1f32..=1.0,
        ) {
            let mut tuning = embedded().map_err(|error| TestCaseError::fail(error.to_string()))?;
            tuning.movement.walk_speed = walk_speed;
            tuning.movement.crouch_speed = walk_speed * 0.5;
            tuning.movement.sprint_speed = walk_speed + 1.0;
            tuning.rifle.damage = damage;
            tuning.bots.sight_m = sight;
            tuning.bots.engage_m = sight * engage_fraction;
            tuning.validate_fields().map_err(TestCaseError::fail)?;
            prop_assert!(tuning.controller_config().is_ok());
            prop_assert!(tuning.weapon_config().is_ok());
            prop_assert!(tuning.bot_config().is_ok());
            prop_assert!(tuning.link_rules().is_ok());
        }
    }
}
