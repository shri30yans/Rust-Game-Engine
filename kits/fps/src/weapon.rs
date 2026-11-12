//! Hitscan weapons: ammunition, reload, fire rate, spread, and the system that resolves shots.
//!
//! [`WeaponState::tick`] is the pure rule. It counts down cooldown and reload, starts a reload on
//! request, fires when allowed, and widens or narrows the spread. A shot is a fixed set of unit
//! directions drawn uniformly from a cone around the aim, stored inline so a shot never allocates.
//!
//! [`weapon_system`] resolves each shot as a ray from the shooter's eye that ignores the shooter's
//! own body. A pellet that hits a locomotion actor on another team (or any actor, when friendly
//! fire is on) queues a [`DamageEvent`]. Walls and floors absorb pellets and deal no damage.
//!
//! Invariants: every config value is validated, so the magazine, reload and fire interval are at
//! least one and the spread never exceeds its maximum; retuning clamps ammunition and spread while
//! preserving in-progress reload and cooldown counters; a shot has between one and [`MAX_PELLETS`]
//! unit directions; a weapon never fires while reloading, cooling down, or empty.

use std::f32::consts::{FRAC_PI_2, TAU};

use engine_core::SimRng;
use engine_physics::{PhysicsWorld, QueryFilter, RayCast};
use engine_world::{NetId, SystemContext, World, WorldError};
use glam::Vec3;
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::command::{Trigger, look_direction};
use crate::controller::Locomotion;
use crate::error::{KitError, lift};
use crate::health::{Damage, DamageEvent, DamageLog, Health, Team, is_alive};
use crate::roster::for_each_with;
use crate::validate::{at_least_one, non_negative, positive};

/// The most pellets one shot may carry. Shots are stored inline up to this count.
pub const MAX_PELLETS: usize = 12;

/// Damage stays full through `full_until_m`, falls linearly, then stays at `min_scale`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DamageFalloff {
    /// Last distance with full damage, in metres.
    pub full_until_m: f32,
    /// Distance where minimum damage is reached, in metres.
    pub min_from_m: f32,
    /// Damage multiplier at and beyond `min_from_m`.
    pub min_scale: f32,
}

impl DamageFalloff {
    /// Damage multiplier for a hit at `distance_m`.
    #[must_use]
    pub fn scale(self, distance_m: f32) -> f32 {
        if distance_m <= self.full_until_m {
            return 1.0;
        }
        if distance_m >= self.min_from_m {
            return self.min_scale;
        }
        let progress = (distance_m - self.full_until_m) / (self.min_from_m - self.full_until_m);
        1.0 + (self.min_scale - 1.0) * progress
    }
}

/// The plain numbers a game passes in for a weapon. [`WeaponConfig::new`] validates them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeaponParams {
    /// Damage of one pellet that hits, in hit points.
    pub damage: u32,
    /// Ticks between two shots, at least one.
    pub fire_interval_ticks: u32,
    /// Rounds in a full magazine, at least one.
    pub magazine: u32,
    /// Ticks a reload takes, at least one.
    pub reload_ticks: u32,
    /// Largest angle from the aim line a pellet may leave at, in radians, in `[0, pi/2)`.
    pub spread_rad: f32,
    /// Spread added by each shot, in radians, zero or more.
    pub spread_per_shot: f32,
    /// Spread removed each tick, in radians, zero or more.
    pub spread_recovery_per_tick: f32,
    /// Longest distance a pellet travels, in metres, greater than zero.
    pub max_range: f32,
    /// Damage reduction by range; required for every weapon.
    pub damage_falloff: DamageFalloff,
    /// Pellets per shot, from one to [`MAX_PELLETS`]. A rifle uses one.
    pub pellets: u32,
    /// Whether pellets damage actors on the shooter's own team.
    pub friendly_fire: bool,
}

/// Validated weapon configuration. Built only from [`WeaponParams`], including when decoded.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "WeaponParams")]
pub struct WeaponConfig {
    damage: Damage,
    fire_interval_ticks: u32,
    magazine: u32,
    reload_ticks: u32,
    spread_rad: f32,
    spread_per_shot: f32,
    spread_recovery_per_tick: f32,
    max_range: f32,
    damage_falloff: DamageFalloff,
    pellets: u8,
    friendly_fire: bool,
    #[serde(skip)]
    query_filter: QueryFilter,
}

impl TryFrom<WeaponParams> for WeaponConfig {
    type Error = KitError;

    fn try_from(params: WeaponParams) -> Result<Self, Self::Error> {
        let spread_rad = non_negative(params.spread_rad, "weapon spread")?;
        if spread_rad >= FRAC_PI_2 {
            return Err(KitError::OutOfRange {
                field: "weapon spread",
                requirement: "less than pi/2",
                value: spread_rad,
            });
        }
        let pellets = at_least_one(params.pellets, "weapon pellets")?;
        let pellets = match u8::try_from(pellets) {
            Ok(count) if usize::from(count) <= MAX_PELLETS => count,
            _ => {
                return Err(KitError::CountOutOfRange {
                    field: "weapon pellets",
                    requirement: "between one and MAX_PELLETS",
                    value: pellets,
                });
            }
        };
        let falloff = params.damage_falloff;
        let full_until_m = positive(falloff.full_until_m, "falloff full damage distance")?;
        let min_from_m = positive(falloff.min_from_m, "falloff minimum damage distance")?;
        let min_scale = positive(falloff.min_scale, "falloff minimum scale")?;
        if full_until_m >= min_from_m || min_from_m > params.max_range || min_scale > 1.0 {
            return Err(KitError::OutOfRange {
                field: "weapon damage falloff",
                requirement: "0 < full_until_m < min_from_m <= max_range and 0 < min_scale <= 1",
                value: min_from_m,
            });
        }
        Ok(Self {
            damage: Damage::new(at_least_one(params.damage, "weapon damage")?),
            fire_interval_ticks: at_least_one(params.fire_interval_ticks, "fire interval")?,
            magazine: at_least_one(params.magazine, "magazine")?,
            reload_ticks: at_least_one(params.reload_ticks, "reload ticks")?,
            spread_rad,
            spread_per_shot: non_negative(params.spread_per_shot, "spread per shot")?,
            spread_recovery_per_tick: non_negative(
                params.spread_recovery_per_tick,
                "spread recovery per tick",
            )?,
            max_range: positive(params.max_range, "weapon max range")?,
            damage_falloff: DamageFalloff {
                full_until_m,
                min_from_m,
                min_scale,
            },
            pellets,
            friendly_fire: params.friendly_fire,
            query_filter: QueryFilter::default(),
        })
    }
}

impl WeaponConfig {
    /// Validates the parameters into a configuration.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] naming the first field that is out of range or not finite.
    pub fn new(params: WeaponParams) -> Result<Self, KitError> {
        Self::try_from(params)
    }

    /// Sets the collider layers selected by hitscan queries.
    #[must_use]
    pub const fn query_filter(mut self, filter: QueryFilter) -> Self {
        self.query_filter = filter;
        self
    }

    /// Returns the hitscan query filter.
    #[must_use]
    pub const fn query_filter_value(&self) -> QueryFilter {
        self.query_filter
    }

    /// Damage of one pellet that hits.
    #[must_use]
    pub const fn damage(&self) -> Damage {
        self.damage
    }

    /// Longest distance a pellet travels, in metres.
    #[must_use]
    pub const fn max_range(&self) -> f32 {
        self.max_range
    }

    /// Damage multiplier for a hit at `distance_m`.
    #[must_use]
    pub fn damage_scale(&self, distance_m: f32) -> f32 {
        self.damage_falloff.scale(distance_m)
    }

    /// Pellets per shot.
    #[must_use]
    pub const fn pellets(&self) -> u8 {
        self.pellets
    }

    /// Whether pellets damage actors on the shooter's own team.
    #[must_use]
    pub const fn friendly_fire(&self) -> bool {
        self.friendly_fire
    }
}

/// The evolving state of one weapon. Its invariants are maintained by [`WeaponState::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeaponState {
    ammo: u32,
    cooldown: u32,
    reload_left: u32,
    spread: f32,
}

impl WeaponState {
    /// A full magazine, ready to fire, with no spread.
    #[must_use]
    pub const fn new(config: &WeaponConfig) -> Self {
        Self {
            ammo: config.magazine,
            cooldown: 0,
            reload_left: 0,
            spread: 0.0,
        }
    }

    /// Rounds left in the magazine.
    #[must_use]
    pub const fn ammo(&self) -> u32 {
        self.ammo
    }

    /// Ticks until the weapon may fire again.
    #[must_use]
    pub const fn cooldown(&self) -> u32 {
        self.cooldown
    }

    /// Ticks left in the current reload, zero when not reloading.
    #[must_use]
    pub const fn reload_left(&self) -> u32 {
        self.reload_left
    }

    /// The current cone half-angle, in radians.
    #[must_use]
    pub const fn spread(&self) -> f32 {
        self.spread
    }

    /// Advances the weapon by one tick and returns the shot fired this tick, if any.
    ///
    /// Order: spread recovers, cooldown counts down, a running reload counts down (and fills the
    /// magazine when it ends), a reload request starts when there is room, then a fire request
    /// fires when not cooling down, not reloading, and not empty. `aim` is the unit direction the
    /// shooter is looking along.
    pub fn tick(
        &mut self,
        config: &WeaponConfig,
        fire: bool,
        reload: bool,
        aim: Vec3,
        rng: &mut SimRng,
    ) -> Option<Shot> {
        self.spread = (self.spread - config.spread_recovery_per_tick).max(0.0);
        self.cooldown = self.cooldown.saturating_sub(1);
        if self.reload_left > 0 {
            self.reload_left = self.reload_left.saturating_sub(1);
            if self.reload_left == 0 {
                self.ammo = config.magazine;
            }
        } else if reload && self.ammo < config.magazine {
            self.reload_left = config.reload_ticks;
        }
        if !fire || self.cooldown > 0 || self.reload_left > 0 || self.ammo == 0 {
            return None;
        }
        let shot = Shot::sample(aim, self.spread, config.pellets, rng);
        self.ammo = self.ammo.saturating_sub(1);
        self.cooldown = config.fire_interval_ticks;
        self.spread = (self.spread + config.spread_per_shot).min(config.spread_rad);
        Some(shot)
    }
}

/// One shot: up to [`MAX_PELLETS`] unit directions, stored inline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    directions: [Vec3; MAX_PELLETS],
    count: u8,
}

impl Shot {
    /// Samples `count` pellet directions uniformly from the cone of half-angle `spread` around
    /// `aim`. With zero spread every pellet leaves along `aim`.
    fn sample(aim: Vec3, spread: f32, count: u8, rng: &mut SimRng) -> Self {
        let live = usize::from(count);
        let directions = std::array::from_fn(|index| {
            if index < live {
                cone_direction(aim, spread, rng)
            } else {
                aim
            }
        });
        Self { directions, count }
    }

    /// The pellet directions, each a unit vector.
    pub fn pellets(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.directions
            .iter()
            .take(usize::from(self.count))
            .copied()
    }

    /// Number of pellets in the shot.
    #[must_use]
    pub const fn count(&self) -> u8 {
        self.count
    }
}

/// A unit direction drawn uniformly from the cone of half-angle `half_angle` around `aim`.
///
/// The cosine of the angle from the axis is uniform on `[cos(half_angle), 1]`, which is uniform
/// over the cone's solid angle; the azimuth is uniform on the circle.
fn cone_direction(aim: Vec3, half_angle: f32, rng: &mut SimRng) -> Vec3 {
    let cos_min = half_angle.cos();
    let cos_theta = 1.0 - (1.0 - cos_min) * rng.next_f32();
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    let azimuth = TAU * rng.next_f32();
    let helper = if aim.y.abs() < 0.999 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let right = aim.cross(helper).normalize();
    let up = right.cross(aim);
    aim * cos_theta + (right * azimuth.cos() + up * azimuth.sin()) * sin_theta
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::command::look_direction;

    const AIM: Vec3 = Vec3::NEG_Z;

    fn params() -> WeaponParams {
        WeaponParams {
            damage: 25,
            fire_interval_ticks: 6,
            magazine: 3,
            reload_ticks: 10,
            spread_rad: 0.05,
            spread_per_shot: 0.01,
            spread_recovery_per_tick: 0.002,
            max_range: 100.0,
            damage_falloff: DamageFalloff {
                full_until_m: 25.0,
                min_from_m: 50.0,
                min_scale: 0.6,
            },
            pellets: 1,
            friendly_fire: false,
        }
    }

    fn rifle() -> Result<WeaponConfig, KitError> {
        WeaponConfig::new(params())
    }

    #[test]
    fn damage_falloff_validates_every_bound() {
        for falloff in [
            DamageFalloff {
                full_until_m: 0.0,
                min_from_m: 50.0,
                min_scale: 0.6,
            },
            DamageFalloff {
                full_until_m: 25.0,
                min_from_m: 0.0,
                min_scale: 0.6,
            },
            DamageFalloff {
                full_until_m: 50.0,
                min_from_m: 50.0,
                min_scale: 0.6,
            },
            DamageFalloff {
                full_until_m: 25.0,
                min_from_m: 101.0,
                min_scale: 0.6,
            },
            DamageFalloff {
                full_until_m: 25.0,
                min_from_m: 50.0,
                min_scale: 0.0,
            },
            DamageFalloff {
                full_until_m: 25.0,
                min_from_m: 50.0,
                min_scale: 1.1,
            },
            DamageFalloff {
                full_until_m: f32::NAN,
                min_from_m: 50.0,
                min_scale: 0.6,
            },
            DamageFalloff {
                full_until_m: 25.0,
                min_from_m: 50.0,
                min_scale: f32::NAN,
            },
        ] {
            assert!(
                WeaponConfig::new(WeaponParams {
                    damage_falloff: falloff,
                    ..params()
                })
                .is_err()
            );
        }
    }

    #[test]
    fn damage_falloff_scales_carbine_hits() -> Result<(), KitError> {
        let mut params = params();
        params.damage = 20;
        let config = WeaponConfig::new(params)?;
        assert_eq!(
            scaled_damage(config.damage(), config.damage_scale(10.0)).amount(),
            20
        );
        assert_eq!(
            scaled_damage(config.damage(), config.damage_scale(37.5)).amount(),
            16
        );
        assert_eq!(
            scaled_damage(config.damage(), config.damage_scale(60.0)).amount(),
            12
        );
        Ok(())
    }

    proptest! {
        #[test]
        fn falloff_scale_is_monotonic_and_bounded(a in 0.0f32..100.0, b in 0.0f32..100.0) {
            let config = rifle().map_err(|e| TestCaseError::fail(e.to_string()))?;
            let near = config.damage_scale(a.min(b));
            let far = config.damage_scale(a.max(b));
            prop_assert!(far <= near);
            prop_assert!((0.6..=1.0).contains(&far));
        }
    }

    fn retune_config(magazine: u32, spread_rad: f32) -> Result<WeaponConfig, KitError> {
        WeaponConfig::new(WeaponParams {
            magazine,
            spread_rad,
            ..params()
        })
    }

    #[test]
    fn retuning_preserves_action_progress_and_clamps_bounded_state() -> Result<(), KitError> {
        let mut weapon = Weapon::new(rifle()?);
        weapon.state = WeaponState {
            ammo: 3,
            cooldown: 4,
            reload_left: 7,
            spread: 0.04,
        };
        weapon.set_config(retune_config(2, 0.02)?);
        assert_eq!(weapon.state.ammo(), 2);
        assert_eq!(weapon.state.cooldown(), 4);
        assert_eq!(weapon.state.reload_left(), 7);
        assert_eq!(weapon.state.spread(), 0.02);
        Ok(())
    }

    proptest! {
        #[test]
        fn retuning_any_generated_valid_config_preserves_weapon_invariants(
            old_ammo in 0u32..100,
            old_spread in 0.0f32..1.0,
            magazine in 1u32..100,
            spread_rad in 0.0f32..1.0,
        ) {
            let config = retune_config(magazine, spread_rad)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let mut weapon = Weapon::new(rifle().map_err(|error| TestCaseError::fail(error.to_string()))?);
            weapon.state = WeaponState {
                ammo: old_ammo.min(weapon.config.magazine),
                cooldown: 5,
                reload_left: 8,
                spread: old_spread.min(weapon.config.spread_rad),
            };
            let cooldown = weapon.state.cooldown;
            let reload = weapon.state.reload_left;
            weapon.set_config(config);
            prop_assert!(weapon.state.ammo <= config.magazine);
            prop_assert!(weapon.state.spread <= config.spread_rad);
            prop_assert_eq!(weapon.state.cooldown, cooldown);
            prop_assert_eq!(weapon.state.reload_left, reload);
        }
    }

    /// One tick of the weapon aimed down -Z, with the given fire and reload requests.
    fn step(
        state: &mut WeaponState,
        config: &WeaponConfig,
        fire: bool,
        reload: bool,
        rng: &mut SimRng,
    ) -> Option<Shot> {
        state.tick(config, fire, reload, AIM, rng)
    }

    #[test]
    fn invalid_parameters_are_rejected() {
        let mut bad = params();
        bad.magazine = 0;
        assert!(WeaponConfig::new(bad).is_err());
        let mut bad = params();
        bad.pellets = 0;
        assert!(WeaponConfig::new(bad).is_err());
        let mut bad = params();
        bad.pellets = 13;
        assert!(WeaponConfig::new(bad).is_err());
        let mut bad = params();
        bad.spread_rad = std::f32::consts::FRAC_PI_2;
        assert!(WeaponConfig::new(bad).is_err());
        let mut bad = params();
        bad.max_range = 0.0;
        assert!(WeaponConfig::new(bad).is_err());
    }

    #[test]
    fn fire_interval_spaces_shots_and_the_magazine_limits_them() -> Result<(), KitError> {
        let cfg = rifle()?;
        let mut state = WeaponState::new(&cfg);
        let mut rng = SimRng::new(1, 0);
        let mut shot_ticks = Vec::new();
        for tick in 0..30 {
            if step(&mut state, &cfg, true, false, &mut rng).is_some() {
                shot_ticks.push(tick);
            }
        }
        assert_eq!(shot_ticks, vec![0, 6, 12]);
        assert_eq!(state.ammo(), 0);
        Ok(())
    }

    #[test]
    fn reload_takes_its_ticks_and_blocks_fire_until_done() -> Result<(), KitError> {
        let cfg = rifle()?;
        let mut state = WeaponState::new(&cfg);
        let mut rng = SimRng::new(1, 0);
        for tick in 0..=12 {
            step(&mut state, &cfg, tick % 6 == 0, false, &mut rng);
        }
        assert_eq!(state.ammo(), 0);
        // The reload request starts a 10 tick reload on tick 13.
        step(&mut state, &cfg, false, true, &mut rng);
        assert_eq!(state.reload_left(), 10);
        for _ in 14..=22 {
            assert!(step(&mut state, &cfg, true, false, &mut rng).is_none());
        }
        assert_eq!(state.ammo(), 0);
        assert_eq!(state.reload_left(), 1);
        // On tick 23 the magazine is full again and the trigger fires.
        assert!(step(&mut state, &cfg, true, false, &mut rng).is_some());
        assert_eq!(state.ammo(), 2);
        assert_eq!(state.reload_left(), 0);
        Ok(())
    }

    #[test]
    fn reload_is_not_started_with_a_full_magazine() -> Result<(), KitError> {
        let cfg = rifle()?;
        let mut state = WeaponState::new(&cfg);
        let mut rng = SimRng::new(1, 0);
        step(&mut state, &cfg, false, true, &mut rng);
        assert_eq!(state.reload_left(), 0);
        Ok(())
    }

    #[test]
    fn spread_grows_with_each_shot_and_recovers_when_idle() -> Result<(), KitError> {
        // Growth per shot outpaces recovery across one fire interval (6 ticks at 0.002 each), so
        // the cone widens shot by shot until the cap.
        let cfg = WeaponConfig::new(WeaponParams {
            spread_per_shot: 0.02,
            ..params()
        })?;
        let mut state = WeaponState::new(&cfg);
        let mut rng = SimRng::new(7, 0);
        let mut spread_after_shots = Vec::new();
        for tick in 0..18 {
            if step(&mut state, &cfg, tick % 6 == 0, false, &mut rng).is_some() {
                spread_after_shots.push(state.spread());
            }
            assert!(state.spread() <= 0.05 + 1e-6);
        }
        assert_eq!(spread_after_shots.len(), 3);
        assert!(
            spread_after_shots
                .windows(2)
                .all(|pair| matches!(pair, [before, after] if after > before))
        );
        for _ in 0..100 {
            step(&mut state, &cfg, false, false, &mut rng);
        }
        assert_eq!(state.spread(), 0.0);
        Ok(())
    }

    #[test]
    fn a_shot_fires_no_more_pellets_than_configured() -> Result<(), KitError> {
        let cfg = WeaponConfig::new(WeaponParams {
            pellets: 12,
            ..params()
        })?;
        let mut state = WeaponState::new(&cfg);
        let mut rng = SimRng::new(3, 0);
        let shot = step(&mut state, &cfg, true, false, &mut rng);
        assert_eq!(shot.map(|s| s.count()), Some(12));
        Ok(())
    }

    #[test]
    fn equal_seeds_give_equal_shots() -> Result<(), KitError> {
        let cfg = WeaponConfig::new(WeaponParams {
            pellets: 8,
            spread_per_shot: 0.02,
            ..params()
        })?;
        let run = |seed: u64| -> Vec<Option<Shot>> {
            let mut state = WeaponState::new(&cfg);
            let mut rng = SimRng::new(seed, 0);
            (0..60)
                .map(|tick| step(&mut state, &cfg, tick % 4 == 0, tick == 30, &mut rng))
                .collect()
        };
        assert_eq!(run(42), run(42));
        Ok(())
    }

    proptest! {
        #[test]
        fn every_pellet_stays_inside_the_current_spread_cone(
            yaw in -3.0f32..=3.0,
            pitch in -1.5f32..=1.5,
            spread in 0.0f32..=1.0,
            seed in any::<u64>(),
        ) {
            let config = WeaponConfig::new(WeaponParams {
                pellets: 12,
                spread_rad: 1.0,
                spread_per_shot: 0.0,
                ..params()
            }).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let view = app::ViewAngles::new(yaw, pitch).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let aim = look_direction(view);
            let mut state = WeaponState::new(&config);
            state.spread = spread;
            let mut rng = SimRng::new(seed, 0);
            let shot = state
                .tick(&config, true, false, aim, &mut rng)
                .ok_or_else(|| TestCaseError::fail("no shot from a ready weapon"))?;
            for dir in shot.pellets() {
                prop_assert!((dir.length() - 1.0).abs() < 1e-3);
                prop_assert!(dir.dot(aim) >= spread.cos() - 1e-4);
            }
        }
    }
}

/// The weapon an entity carries: its configuration and its state. Systems read the config each
/// tick; the state is part of the state hash.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Weapon {
    config: WeaponConfig,
    state: WeaponState,
}

impl Weapon {
    /// A weapon with a full magazine.
    #[must_use]
    pub const fn new(config: WeaponConfig) -> Self {
        let state = WeaponState::new(&config);
        Self { config, state }
    }

    /// The weapon's configuration.
    #[must_use]
    pub const fn config(&self) -> &WeaponConfig {
        &self.config
    }

    /// Replaces the validated config while preserving the weapon's current activity.
    ///
    /// Ammunition is clamped to the new magazine size and spread to its new maximum. Reload and
    /// cooldown counters are retained so a retune cannot interrupt an action already in progress.
    pub fn set_config(&mut self, config: WeaponConfig) {
        self.state.ammo = self.state.ammo.min(config.magazine);
        self.state.spread = self.state.spread.min(config.spread_rad);
        self.config = config;
    }

    /// The weapon's state after the last tick.
    #[must_use]
    pub const fn state(&self) -> WeaponState {
        self.state
    }

    /// Replaces the state after a tick.
    pub(crate) const fn set_state(&mut self, state: WeaponState) {
        self.state = state;
    }
}

/// Resolves every weapon's trigger for one tick: a shot is fired from the eye along each pellet
/// direction, and each pellet that hits an actor queues a [`DamageEvent`] in the [`DamageLog`].
///
/// Each shooter needs a [`Locomotion`] (for its eye, body and command) and a [`Team`]. Dead
/// shooters do not fire or reload. Spread draws come from the system's own RNG stream.
///
/// # Errors
///
/// Returns a world error when a shooter lacks a locomotion or team, the physics world or damage
/// log is missing, or a ray cannot be cast.
pub fn weapon_system(world: &mut World, ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let rng = &mut *ctx.rng;
    for_each_with::<Weapon, _>(world, |world, shooter| fire(world, shooter, rng))
}

/// Advances one shooter's weapon by one tick and resolves any shot it fires.
fn fire(world: &mut World, shooter: NetId, rng: &mut SimRng) -> Result<(), WorldError> {
    let weapon = *world.get::<Weapon>(shooter)?;
    let locomotion = *world.get::<Locomotion>(shooter)?;
    let team = *world.get::<Team>(shooter)?;
    let alive = is_alive(world, shooter)?;
    let command = locomotion.command();
    let aim = look_direction(command.view());
    let firing = match command.trigger() {
        Trigger::Released => false,
        Trigger::Held => true,
        Trigger::OnTarget => {
            alive && enemy_first_in_aim_ray(world, shooter, &locomotion, team, &weapon)?
        }
    };
    let mut state = weapon.state();
    let shot = state.tick(
        weapon.config(),
        alive && firing,
        alive && command.reload(),
        aim,
        rng,
    );
    world.get_mut::<Weapon>(shooter)?.set_state(state);
    let Some(shot) = shot else {
        return Ok(());
    };

    let mut targets: [Option<NetId>; MAX_PELLETS] = [None; MAX_PELLETS];
    {
        let physics = world.resource::<PhysicsWorld>()?;
        for (slot, direction) in targets.iter_mut().zip(shot.pellets()) {
            let request = lift(RayCast::new(
                locomotion.eye(),
                direction,
                weapon.config().max_range(),
                true,
            ))?;
            let mut filter = weapon.config().query_filter_value();
            filter.exclude_body = Some(locomotion.body());
            let hit = physics.ray_cast(request, filter);
            if let Some(body) = hit.and_then(|hit| hit.owner.body) {
                *slot = locomotion_of_body(world, body);
            }
        }
    }

    for target in targets.into_iter().flatten() {
        if world.get::<Health>(target).is_err() {
            continue;
        }
        let target_team = *world.get::<Team>(target)?;
        if target_team == team && !weapon.config().friendly_fire() {
            continue;
        }
        let origin = locomotion.eye();
        let target_centre = world.get::<Locomotion>(target)?.centre();
        let distance = origin.distance(target_centre);
        let damage = scaled_damage(
            weapon.config().damage(),
            weapon.config().damage_scale(distance),
        );
        world
            .resource_mut::<DamageLog>()?
            .push(DamageEvent::new(target, shooter, damage));
    }
    Ok(())
}

/// Applies a validated scale to whole-hit-point damage, rounding down deterministically.
fn scaled_damage(damage: Damage, scale: f32) -> Damage {
    let scaled = f64::from(damage.amount()) * f64::from(scale);
    Damage::new(scaled.floor().to_u32().unwrap_or(damage.amount()))
}

/// Resolves an `OnTarget` trigger with the same first-hit ray the weapon uses this tick.
fn enemy_first_in_aim_ray(
    world: &World,
    shooter: NetId,
    locomotion: &Locomotion,
    team: Team,
    weapon: &Weapon,
) -> Result<bool, WorldError> {
    let request = lift(RayCast::new(
        locomotion.eye(),
        look_direction(locomotion.command().view()),
        weapon.config().max_range(),
        true,
    ))?;
    let physics = world.resource::<PhysicsWorld>()?;
    let mut filter = weapon.config().query_filter_value();
    filter.exclude_body = Some(locomotion.body());
    let hit = physics.ray_cast(request, filter);
    let Some(body) = hit.and_then(|hit| hit.owner.body) else {
        return Ok(false);
    };
    let Some(target) = locomotion_of_body(world, body) else {
        return Ok(false);
    };
    if target == shooter || !is_alive(world, target)? {
        return Ok(false);
    }
    Ok(*world.get::<Team>(target)? != team)
}

/// The locomotion actor whose body is `body`, if any. Scans the world without allocating.
fn locomotion_of_body(world: &World, body: engine_physics::BodyId) -> Option<NetId> {
    world
        .net_ids()
        .find(|id| matches!(world.get::<Locomotion>(*id), Ok(loco) if loco.body() == body))
}
