//! Allocation-free match combat measurements. Install with the actor ids, then run
//! [`combat_stats_system`] after the damage pass: it reads the tick's [`ShotFired`] and [`Damaged`]
//! events and each bot's sight state.
//!
//! Invariants: the stats only read the world and the event logs, so the system can run in any
//! order among the other readers of the tick's events; a death is timed once, at the hit that
//! reported it.

use engine_core::{StateHasher, Tick};
use engine_world::{NetId, Resource, SystemContext, World, WorldError};
use num_traits::ToPrimitive;

use crate::events::{Damaged, ShotFired};
use crate::{Bot, Locomotion, Outcome, Phase};

const DISTANCE_BUCKET_M: f32 = 5.0;
const DISTANCE_BUCKETS: usize = 64;
const REACTION_WINDOW_TICKS: u32 = 30;

/// Per actor counters required to update combat metrics without per-tick allocation.
#[derive(Debug, Clone)]
struct ActorStats {
    id: NetId,
    shots: u64,
    hits: u64,
    first_sighting: Option<Tick>,
    first_sighting_distance_m: Option<f32>,
    first_engage_range: Option<Tick>,
    first_hit: Option<Tick>,
    first_hit_distance_m: Option<f32>,
    first_damage: Option<Tick>,
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
    /// Creates metrics for fixed actor ids.
    #[must_use]
    pub fn new(actors: &[NetId]) -> Self {
        Self {
            actors: actors
                .iter()
                .map(|id| ActorStats {
                    id: *id,
                    shots: 0,
                    hits: 0,
                    first_sighting: None,
                    first_sighting_distance_m: None,
                    first_engage_range: None,
                    first_hit: None,
                    first_hit_distance_m: None,
                    first_damage: None,
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

/// Reads this tick's shots and applied hits, and each bot's sight state.
///
/// # Errors
///
/// Returns a world error when tracked actors, required resources or event logs are missing.
pub fn combat_stats_system(
    world: &mut World,
    ctx: &mut SystemContext<'_>,
) -> Result<(), WorldError> {
    let tick = ctx.tick;
    record_sightings(world, tick)?;
    record_shots(world)?;
    let event_count = world.events::<Damaged>()?.len();
    for event_index in 0..event_count {
        let Some(event) = world.events::<Damaged>()?.get(event_index).copied() else {
            continue;
        };
        record_hit(world.resource_mut::<CombatStats>()?, tick, event);
    }
    let metrics = &mut world.resource_mut::<CombatStats>()?;
    for actor in &mut metrics.actors {
        if actor.shot_this_tick && actor.hit_this_tick {
            actor.hits = actor.hits.saturating_add(1);
            metrics.total_hits = metrics.total_hits.saturating_add(1);
        }
    }
    Ok(())
}

/// Clears each actor's per-tick flags and records a bot's first sighting and first time in range.
fn record_sightings(world: &mut World, tick: Tick) -> Result<(), WorldError> {
    let count = world.resource::<CombatStats>()?.actors.len();
    for index in 0..count {
        let id = world
            .resource::<CombatStats>()?
            .actors
            .get(index)
            .ok_or(WorldError::Invariant("combat actor index is out of range"))?
            .id;
        let sighting = world.get::<Bot>(id).ok().and_then(|bot| {
            let brain = bot.brain();
            if !brain.is_sighted() {
                return None;
            }
            let target = brain.last_known_enemy_position()?;
            let distance = world.get::<Locomotion>(id).ok()?.eye().distance(target);
            Some((distance, brain.phase() == Phase::Attack))
        });
        let metrics = world.resource_mut::<CombatStats>()?;
        let stats = metrics
            .actors
            .get_mut(index)
            .ok_or(WorldError::Invariant("combat actor index is out of range"))?;
        stats.shot_this_tick = false;
        stats.hit_this_tick = false;
        stats.died_this_tick = false;
        let Some((distance, in_engage_range)) = sighting else {
            continue;
        };
        if in_engage_range && stats.first_engage_range.is_none() {
            stats.first_engage_range = Some(tick);
        }
        if stats.first_sighting.is_none() {
            stats.first_sighting = Some(tick);
            stats.first_sighting_distance_m = Some(distance);
            metrics.first_sighting_distance_m += f64::from(distance);
            metrics.first_sighting_distance_count =
                metrics.first_sighting_distance_count.saturating_add(1);
        }
    }
    Ok(())
}

/// Counts each shot fired this tick against its shooter.
fn record_shots(world: &mut World) -> Result<(), WorldError> {
    let count = world.events::<ShotFired>()?.len();
    for index in 0..count {
        let Some(shot) = world.events::<ShotFired>()?.get(index).copied() else {
            continue;
        };
        let metrics = world.resource_mut::<CombatStats>()?;
        let Some(stats) = metrics
            .actors
            .iter_mut()
            .find(|actor| actor.id == shot.shooter)
        else {
            continue;
        };
        stats.shots = stats.shots.saturating_add(1);
        stats.shot_this_tick = true;
        if stats.first_damage.is_some() {
            stats.fired_after_damage = true;
        }
        metrics.total_shots = metrics.total_shots.saturating_add(1);
    }
    Ok(())
}

/// Records one applied hit: the shooter's first hit and its timings, the distance histogram, and
/// for the target the time from first damage to death.
fn record_hit(metrics: &mut CombatStats, tick: Tick, event: Damaged) {
    let hit = event.hit;
    let distance = hit.distance_m();
    if let Some(actor) = metrics
        .actors
        .iter_mut()
        .find(|actor| actor.id == hit.source())
    {
        actor.hit_this_tick = true;
        if actor.first_hit.is_none() {
            actor.first_hit = Some(tick);
            actor.first_hit_distance_m = Some(distance);
            let sighting_elapsed = actor
                .first_sighting
                .map(|seen| tick.raw().saturating_sub(seen.raw()));
            let engage_elapsed = actor
                .first_engage_range
                .map(|entered| tick.raw().saturating_sub(entered.raw()));
            if let Some(elapsed) = sighting_elapsed {
                metrics.first_sighting_to_hit_ticks = metrics
                    .first_sighting_to_hit_ticks
                    .saturating_add(u64::from(elapsed));
                metrics.first_sighting_to_hit_count =
                    metrics.first_sighting_to_hit_count.saturating_add(1);
            }
            if let Some(elapsed) = engage_elapsed {
                metrics.engage_to_hit_ticks = metrics
                    .engage_to_hit_ticks
                    .saturating_add(u64::from(elapsed));
                metrics.engage_to_hit_count = metrics.engage_to_hit_count.saturating_add(1);
            }
            metrics.first_hit_distance_m += f64::from(distance);
            metrics.first_hit_distance_count = metrics.first_hit_distance_count.saturating_add(1);
        }
        let bucket = (distance / DISTANCE_BUCKET_M)
            .floor()
            .to_usize()
            .unwrap_or(DISTANCE_BUCKETS - 1)
            .min(DISTANCE_BUCKETS - 1);
        if let Some(count) = metrics.distances.get_mut(bucket) {
            *count = count.saturating_add(1);
        }
    }
    let Some(stats) = metrics
        .actors
        .iter_mut()
        .find(|actor| actor.id == hit.target())
    else {
        return;
    };
    if stats.died_this_tick {
        return;
    }
    if stats.first_damage.is_none() {
        stats.first_damage = Some(tick);
    }
    if event.outcome != Outcome::Died {
        return;
    }
    stats.died_this_tick = true;
    let start = stats.first_damage.take();
    let reacted = std::mem::take(&mut stats.fired_after_damage);
    if let Some(start) = start {
        let elapsed = tick.raw().saturating_sub(start.raw());
        metrics.time_to_kill_ticks = metrics
            .time_to_kill_ticks
            .saturating_add(u64::from(elapsed));
        metrics.time_to_kill_count = metrics.time_to_kill_count.saturating_add(1);
        if elapsed < REACTION_WINDOW_TICKS && !reacted {
            metrics.deaths_before_reacting = metrics.deaths_before_reacting.saturating_add(1);
        }
    }
}
