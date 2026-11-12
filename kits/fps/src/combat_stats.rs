//! Allocation-free match combat measurements. Install with actor ids and initial ammo, then run
//! [`combat_stats_system`] after weapons and before damage so it can observe this tick's log.

use engine_core::{StateHasher, Tick};
use engine_world::{NetId, Resource, SystemContext, World, WorldError};
use num_traits::ToPrimitive;

use crate::{Bot, DamageLog, Health, Locomotion, Phase, Weapon};

const DISTANCE_BUCKET_M: f32 = 5.0;
const DISTANCE_BUCKETS: usize = 64;
const REACTION_WINDOW_TICKS: u32 = 30;

/// Per actor counters required to update combat metrics without per-tick allocation.
#[derive(Debug, Clone)]
struct ActorStats {
    id: NetId,
    last_ammo: u32,
    shots: u64,
    hits: u64,
    first_sighting: Option<Tick>,
    first_sighting_distance_m: Option<f32>,
    first_engage_range: Option<Tick>,
    first_hit: Option<Tick>,
    first_hit_distance_m: Option<f32>,
    first_damage: Option<Tick>,
    damage_taken_this_tick: u32,
    fired_after_damage: bool,
    shot_this_tick: bool,
    hit_this_tick: bool,
    died_this_tick: bool,
}

/// Match combat measurements, sized once for the actor roster.
pub struct CombatStats {
    actors: Vec<ActorStats>,
    distances: [u64; DISTANCE_BUCKETS],
    total_shots: u64,
    total_hits: u64,
    first_sighting_to_hit_ticks: u64,
    first_sighting_to_hit_count: u64,
    first_sighting_distance_m: f64,
    first_sighting_distance_count: u64,
    first_hit_distance_m: f64,
    first_hit_distance_count: u64,
    engage_to_hit_ticks: u64,
    engage_to_hit_count: u64,
    time_to_kill_ticks: u64,
    time_to_kill_count: u64,
    deaths_before_reacting: u64,
}

impl CombatStats {
    /// Creates metrics for fixed actor ids and their starting ammunition.
    #[must_use]
    pub fn new(actors: &[(NetId, u32)]) -> Self {
        Self {
            actors: actors
                .iter()
                .map(|(id, ammo)| ActorStats {
                    id: *id,
                    last_ammo: *ammo,
                    shots: 0,
                    hits: 0,
                    first_sighting: None,
                    first_sighting_distance_m: None,
                    first_engage_range: None,
                    first_hit: None,
                    first_hit_distance_m: None,
                    first_damage: None,
                    damage_taken_this_tick: 0,
                    fired_after_damage: false,
                    shot_this_tick: false,
                    hit_this_tick: false,
                    died_this_tick: false,
                })
                .collect(),
            distances: [0; DISTANCE_BUCKETS],
            total_shots: 0,
            total_hits: 0,
            first_sighting_to_hit_ticks: 0,
            first_sighting_to_hit_count: 0,
            first_sighting_distance_m: 0.0,
            first_sighting_distance_count: 0,
            first_hit_distance_m: 0.0,
            first_hit_distance_count: 0,
            engage_to_hit_ticks: 0,
            engage_to_hit_count: 0,
            time_to_kill_ticks: 0,
            time_to_kill_count: 0,
            deaths_before_reacting: 0,
        }
    }

    /// Total weapon shots fired by all tracked actors.
    #[must_use]
    pub const fn shots_fired(&self) -> u64 {
        self.total_shots
    }
    /// Total hit events logged by all tracked actors.
    #[must_use]
    pub const fn hits(&self) -> u64 {
        self.total_hits
    }
    /// Hit percentage, or zero before any shots.
    #[must_use]
    pub fn hit_percentage(&self) -> f32 {
        if self.total_shots == 0 {
            return 0.0;
        }
        let percentage = 100.0 * self.total_hits.to_f64().unwrap_or(0.0)
            / self.total_shots.to_f64().unwrap_or(1.0);
        percentage.to_f32().unwrap_or(0.0)
    }
    /// Upper edge of the median five metre hit-distance bucket.
    #[must_use]
    pub fn median_engagement_distance_m(&self) -> Option<f32> {
        let total: u64 = self.distances.iter().sum();
        if total == 0 {
            return None;
        }
        let median_rank = (total - 1) / 2;
        let mut seen = 0_u64;
        for (index, count) in self.distances.iter().enumerate() {
            seen += count;
            if seen > median_rank {
                return Some(
                    (f32::from(u16::try_from(index).unwrap_or(u16::MAX)) + 1.0) * DISTANCE_BUCKET_M,
                );
            }
        }
        None
    }
    /// Mean ticks from first sighting to first hit, for actors with both events.
    #[must_use]
    pub fn mean_sighting_to_hit_ticks(&self) -> Option<f32> {
        (self.first_sighting_to_hit_count > 0).then(|| {
            (self.first_sighting_to_hit_ticks.to_f64().unwrap_or(0.0)
                / self.first_sighting_to_hit_count.to_f64().unwrap_or(1.0))
            .to_f32()
            .unwrap_or(0.0)
        })
    }
    /// Mean distance when bots first saw an enemy, in metres.
    #[must_use]
    pub fn mean_first_sighting_distance_m(&self) -> Option<f32> {
        (self.first_sighting_distance_count > 0).then(|| {
            (self.first_sighting_distance_m
                / self.first_sighting_distance_count.to_f64().unwrap_or(1.0))
            .to_f32()
            .unwrap_or(0.0)
        })
    }
    /// Mean distance for each actor's first hit, in metres.
    #[must_use]
    pub fn mean_first_hit_distance_m(&self) -> Option<f32> {
        (self.first_hit_distance_count > 0).then(|| {
            (self.first_hit_distance_m / self.first_hit_distance_count.to_f64().unwrap_or(1.0))
                .to_f32()
                .unwrap_or(0.0)
        })
    }
    /// Mean ticks from first entering visible engage range to the first hit.
    #[must_use]
    pub fn mean_engage_to_hit_ticks(&self) -> Option<f32> {
        (self.engage_to_hit_count > 0).then(|| {
            (self.engage_to_hit_ticks.to_f64().unwrap_or(0.0)
                / self.engage_to_hit_count.to_f64().unwrap_or(1.0))
            .to_f32()
            .unwrap_or(0.0)
        })
    }
    /// Mean ticks from first damage to a kill, for completed kills.
    #[must_use]
    pub fn mean_time_to_kill_ticks(&self) -> Option<f32> {
        (self.time_to_kill_count > 0).then(|| {
            (self.time_to_kill_ticks.to_f64().unwrap_or(0.0)
                / self.time_to_kill_count.to_f64().unwrap_or(1.0))
            .to_f32()
            .unwrap_or(0.0)
        })
    }
    /// Deaths within 0.5 seconds of first damage without firing back.
    #[must_use]
    pub const fn deaths_before_reacting(&self) -> u64 {
        self.deaths_before_reacting
    }
}

impl Resource for CombatStats {
    fn hash_into(&self, _hasher: &mut StateHasher) {}
}

/// Reads ammo changes, bot sight state and the damage log for this tick.
///
/// # Errors
///
/// Returns a world error when tracked actors or required resources are missing.
pub fn combat_stats_system(
    world: &mut World,
    ctx: &mut SystemContext<'_>,
) -> Result<(), WorldError> {
    let tick = ctx.tick;
    let count = world.resource::<CombatStats>()?.actors.len();
    for index in 0..count {
        let id = world
            .resource::<CombatStats>()?
            .actors
            .get(index)
            .ok_or(WorldError::Invariant("combat actor index is out of range"))?
            .id;
        let ammo = world.get::<Weapon>(id)?.state().ammo();
        let sighting = world.get::<Bot>(id).ok().and_then(|bot| {
            let brain = bot.brain();
            if !brain.is_sighted() {
                return None;
            }
            let target = brain.last_known_enemy_position()?;
            let distance = world.get::<Locomotion>(id).ok()?.eye().distance(target);
            Some((distance, brain.phase() == Phase::Attack))
        });
        let (shot, new_sighting_distance) = {
            let stats = world
                .resource_mut::<CombatStats>()?
                .actors
                .get_mut(index)
                .ok_or(WorldError::Invariant("combat actor index is out of range"))?;
            stats.shot_this_tick = false;
            stats.hit_this_tick = false;
            stats.died_this_tick = false;
            let fired = ammo < stats.last_ammo;
            stats.last_ammo = ammo;
            if fired {
                stats.shots = stats.shots.saturating_add(1);
                stats.shot_this_tick = true;
                if stats.first_damage.is_some() {
                    stats.fired_after_damage = true;
                }
            }
            let new_sighting_distance = if let Some((distance, in_engage_range)) = sighting {
                let first_sighting_distance = if stats.first_sighting.is_none() {
                    stats.first_sighting = Some(tick);
                    stats.first_sighting_distance_m = Some(distance);
                    Some(distance)
                } else {
                    None
                };
                if in_engage_range && stats.first_engage_range.is_none() {
                    stats.first_engage_range = Some(tick);
                }
                first_sighting_distance
            } else {
                None
            };
            (fired, new_sighting_distance)
        };
        if let Some(distance) = new_sighting_distance {
            let metrics = world.resource_mut::<CombatStats>()?;
            metrics.first_sighting_distance_m += f64::from(distance);
            metrics.first_sighting_distance_count =
                metrics.first_sighting_distance_count.saturating_add(1);
        }
        if shot {
            let metrics = world.resource_mut::<CombatStats>()?;
            metrics.total_shots = metrics.total_shots.saturating_add(1);
        }
    }
    let event_count = world.resource::<DamageLog>()?.events().len();
    for event_index in 0..event_count {
        let Some(event) = world
            .resource::<DamageLog>()?
            .events()
            .get(event_index)
            .copied()
        else {
            continue;
        };
        let source_index = world
            .resource::<CombatStats>()?
            .actors
            .iter()
            .position(|actor| actor.id == event.source());
        let target_index = world
            .resource::<CombatStats>()?
            .actors
            .iter()
            .position(|actor| actor.id == event.target());
        let distance = world
            .get::<Locomotion>(event.source())?
            .eye()
            .distance(world.get::<Locomotion>(event.target())?.centre());
        if let Some(index) = source_index {
            let (sighting_elapsed, engage_elapsed, first_hit_distance) = {
                let metrics = world.resource_mut::<CombatStats>()?;
                let actor = metrics
                    .actors
                    .get_mut(index)
                    .ok_or(WorldError::Invariant("combat source index is out of range"))?;
                actor.hit_this_tick = true;
                let first_hit = actor.first_hit.is_none();
                let elapsed = if first_hit {
                    actor.first_hit = Some(tick);
                    actor.first_hit_distance_m = Some(distance);
                    actor
                        .first_sighting
                        .map(|seen| tick.raw().saturating_sub(seen.raw()))
                } else {
                    None
                };
                let engage_elapsed = if first_hit {
                    actor
                        .first_engage_range
                        .map(|entered| tick.raw().saturating_sub(entered.raw()))
                } else {
                    None
                };
                let first_hit_distance = first_hit.then_some(actor.first_hit_distance_m).flatten();
                let bucket = (distance / DISTANCE_BUCKET_M)
                    .floor()
                    .to_usize()
                    .unwrap_or(DISTANCE_BUCKETS - 1)
                    .min(DISTANCE_BUCKETS - 1);
                if let Some(count) = metrics.distances.get_mut(bucket) {
                    *count = count.saturating_add(1);
                }
                (elapsed, engage_elapsed, first_hit_distance)
            };
            if let Some(elapsed) = sighting_elapsed {
                let metrics = world.resource_mut::<CombatStats>()?;
                metrics.first_sighting_to_hit_ticks = metrics
                    .first_sighting_to_hit_ticks
                    .saturating_add(u64::from(elapsed));
                metrics.first_sighting_to_hit_count =
                    metrics.first_sighting_to_hit_count.saturating_add(1);
            }
            if let Some(elapsed) = engage_elapsed {
                let metrics = world.resource_mut::<CombatStats>()?;
                metrics.engage_to_hit_ticks = metrics
                    .engage_to_hit_ticks
                    .saturating_add(u64::from(elapsed));
                metrics.engage_to_hit_count = metrics.engage_to_hit_count.saturating_add(1);
            }
            if let Some(distance) = first_hit_distance {
                let metrics = world.resource_mut::<CombatStats>()?;
                metrics.first_hit_distance_m += f64::from(distance);
                metrics.first_hit_distance_count =
                    metrics.first_hit_distance_count.saturating_add(1);
            }
        }
        if let Some(index) = target_index {
            let current = world.get::<Health>(event.target())?.current();
            let (first, lethal_now, reacted) = {
                let stats = world
                    .resource_mut::<CombatStats>()?
                    .actors
                    .get_mut(index)
                    .ok_or(WorldError::Invariant("combat target index is out of range"))?;
                if stats.died_this_tick {
                    (stats.first_damage, false, stats.fired_after_damage)
                } else {
                    if stats.first_damage.is_none() {
                        stats.first_damage = Some(tick);
                    }
                    let was_lethal = stats.damage_taken_this_tick >= current;
                    stats.damage_taken_this_tick = stats
                        .damage_taken_this_tick
                        .saturating_add(event.amount().amount());
                    let lethal_now = !was_lethal && stats.damage_taken_this_tick >= current;
                    if lethal_now {
                        stats.died_this_tick = true;
                    }
                    (stats.first_damage, lethal_now, stats.fired_after_damage)
                }
            };
            if lethal_now && let Some(start) = first {
                let elapsed = tick.raw().saturating_sub(start.raw());
                let metrics = world.resource_mut::<CombatStats>()?;
                metrics.time_to_kill_ticks = metrics
                    .time_to_kill_ticks
                    .saturating_add(u64::from(elapsed));
                metrics.time_to_kill_count = metrics.time_to_kill_count.saturating_add(1);
                if elapsed < REACTION_WINDOW_TICKS && !reacted {
                    metrics.deaths_before_reacting =
                        metrics.deaths_before_reacting.saturating_add(1);
                }
                let actor = metrics
                    .actors
                    .get_mut(index)
                    .ok_or(WorldError::Invariant("combat target index is out of range"))?;
                actor.first_damage = None;
                actor.fired_after_damage = false;
            }
        }
    }
    let metrics = &mut world.resource_mut::<CombatStats>()?;
    for actor in &mut metrics.actors {
        if actor.shot_this_tick && actor.hit_this_tick {
            actor.hits = actor.hits.saturating_add(1);
            metrics.total_hits = metrics.total_hits.saturating_add(1);
        }
        actor.damage_taken_this_tick = 0;
    }
    Ok(())
}
