//! The game rules: who is down, when and where each actor respawns, and the player's score.
//!
//! Two systems run once per tick after the kit's damage pass. Each actor is [`Life::Alive`] or
//! [`Life::Down`] with a countdown:
//! - [`respawn_system`] (a writer, before the readers of the tick's events) counts the downed
//!   actors down and brings back those that are due. A respawn restores health, ammunition and
//!   locomotion, resets a bot's brain, and places the physics body at the chosen spawn point.
//! - [`rules_system`] (one of the readers of the kit's [`Killed`] events, in any order among them)
//!   puts each victim down and scores the kill. Only the transition from alive to down scores, so
//!   a death is counted once.
//!
//! Invariants: actor 0 is the player and the rest are bots (the slot order is fixed); only this
//! module changes an actor's life; the spawn choice is a pure function
//! of positions and the lowest index wins a tie; the score is only ever incremented.

use engine_core::StateHasher;
use engine_physics::{BodyId, ConvexShape, PhysicsWorld};
use engine_world::{NetId, Resource, SystemContext, World, WorldError};
use glam::{Quat, Vec3};
use kit_fps::{Bot, Health, Killed, Locomotion, Weapon};

use crate::data::{CAPSULE_RADIUS_M, EYE_ABOVE_CENTRE_M, Tuning, ms_to_ticks};
use crate::error::downstream;
use crate::scene::ArenaData;

/// Slot of the player in [`Actors`].
pub const PLAYER_SLOT: usize = 0;
/// Number of rows in the fixed HUD kill-feed document.
pub const KILL_FEED_ROWS: usize = 4;

/// Whether an actor is in play, and if not, how long until it respawns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Life {
    /// In play.
    Alive,
    /// Down. Respawns when `ticks_left` reaches zero.
    Down {
        /// Ticks until the respawn.
        ticks_left: u32,
    },
}

/// One actor: its entity, its physics body and its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actor {
    id: NetId,
    body: BodyId,
    life: Life,
    respawns: u32,
    stream: u64,
}

impl Actor {
    /// A living actor that has not yet respawned. `stream` names its RNG stream for bot brains.
    #[must_use]
    pub const fn new(id: NetId, body: BodyId, stream: u64) -> Self {
        Self {
            id,
            body,
            life: Life::Alive,
            respawns: 0,
            stream,
        }
    }

    /// The actor's entity.
    #[must_use]
    pub const fn id(self) -> NetId {
        self.id
    }

    /// The actor's physics body.
    #[must_use]
    pub const fn body(self) -> BodyId {
        self.body
    }

    /// The actor's life.
    #[must_use]
    pub const fn life(self) -> Life {
        self.life
    }

    /// Whether the actor is in play.
    #[must_use]
    pub const fn is_alive(self) -> bool {
        matches!(self.life, Life::Alive)
    }

    /// How many times the actor has respawned.
    #[must_use]
    pub const fn respawns(self) -> u32 {
        self.respawns
    }
}

/// The actors in slot order: the player at [`PLAYER_SLOT`], then the bots. A resource.
pub struct Actors {
    slots: Vec<Actor>,
}

impl Actors {
    /// The actors in slot order.
    ///
    /// # Errors
    ///
    /// Returns a world error when the player slot is absent.
    pub fn new(slots: Vec<Actor>) -> Result<Self, WorldError> {
        if slots.is_empty() {
            return Err(WorldError::Invariant("the arena has no player actor"));
        }
        Ok(Self { slots })
    }

    /// The actor in `slot`, or `None` when the slot does not exist.
    #[must_use]
    pub fn get(&self, slot: usize) -> Option<Actor> {
        self.slots.get(slot).copied()
    }

    /// Every actor, in slot order.
    #[must_use]
    pub fn slots(&self) -> &[Actor] {
        &self.slots
    }

    /// Adds a newly joined actor at the end of the stable actor order.
    pub fn push(&mut self, actor: Actor) {
        self.slots.push(actor);
    }

    /// The slot of the actor whose entity is `id`, if it is an actor.
    #[must_use]
    pub fn slot_of(&self, id: NetId) -> Option<usize> {
        self.slots.iter().position(|actor| actor.id == id)
    }

    /// Mutable access to one actor, or `None` when the slot does not exist.
    fn slot_mut(&mut self, slot: usize) -> Option<&mut Actor> {
        self.slots.get_mut(slot)
    }
}

impl Resource for Actors {
    fn hash_into(&self, hasher: &mut StateHasher) {
        for actor in &self.slots {
            hasher.write_u32(actor.id.index());
            hasher.write_u32(actor.id.generation());
            hasher.write_u64(actor.stream);
            hasher.write_u32(actor.respawns);
            match actor.life {
                Life::Alive => hasher.write_u8(0),
                Life::Down { ticks_left } => {
                    hasher.write_u8(1);
                    hasher.write_u32(ticks_left);
                }
            }
        }
    }
}

/// One actor's kills and deaths.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    kills: u32,
    deaths: u32,
}

impl Tally {
    /// Kills credited to the actor.
    #[must_use]
    pub fn kills(&self) -> u32 {
        self.kills
    }

    /// Times the actor went down.
    #[must_use]
    pub fn deaths(&self) -> u32 {
        self.deaths
    }
}

/// The match score: every actor's kills and deaths, and how many ticks the match has run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Score {
    tallies: Vec<Tally>,
    ticks: u32,
}

impl Score {
    /// Builds one tally per actor slot.
    #[must_use]
    pub fn with_actor_count(count: usize) -> Self {
        Self {
            tallies: vec![Tally::default(); count],
            ticks: 0,
        }
    }
    /// Kills by the player.
    #[must_use]
    pub fn kills(&self) -> u32 {
        self.tallies.first().map_or(0, |tally| tally.kills)
    }

    /// Deaths of the player.
    #[must_use]
    pub fn deaths(&self) -> u32 {
        self.tallies.first().map_or(0, |tally| tally.deaths)
    }

    /// The tally of the actor in `slot`, or `None` for a slot that does not exist.
    #[must_use]
    pub fn tally(&self, slot: usize) -> Option<Tally> {
        self.tallies.get(slot).copied()
    }

    /// Tallies for all actor slots.
    #[must_use]
    pub fn tallies(&self) -> &[Tally] {
        &self.tallies
    }

    /// Ticks the match has run.
    #[must_use]
    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    fn tally_mut(&mut self, slot: usize) -> Option<&mut Tally> {
        self.tallies.get_mut(slot)
    }
}

impl Resource for Score {
    fn hash_into(&self, hasher: &mut StateHasher) {
        for tally in &self.tallies {
            hasher.write_u32(tally.kills);
            hasher.write_u32(tally.deaths);
        }
        hasher.write_u32(self.ticks);
    }
}

/// One line of the kill feed: who downed whom, and how long the line stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedEntry {
    killer: usize,
    victim: usize,
    ticks_left: u32,
}

impl FeedEntry {
    /// The killer's actor slot.
    #[must_use]
    pub const fn killer(self) -> usize {
        self.killer
    }

    /// The victim's actor slot.
    #[must_use]
    pub const fn victim(self) -> usize {
        self.victim
    }
}

/// The latest kills, newest first. A resource.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct KillFeed {
    rows: Vec<Option<FeedEntry>>,
}

impl KillFeed {
    /// Creates a feed with the configured row count.
    #[must_use]
    pub fn with_rows(rows: usize) -> Self {
        Self {
            rows: vec![None; rows],
        }
    }

    /// The lines, newest first; a `None` row is empty.
    #[must_use]
    pub fn rows(&self) -> &[Option<FeedEntry>] {
        &self.rows
    }

    /// Puts a kill at the top, pushing the older lines down and dropping the oldest.
    fn push(&mut self, entry: FeedEntry) {
        self.rows.rotate_right(1);
        if let Some(first) = self.rows.first_mut() {
            *first = Some(entry);
        }
    }

    /// Counts every line down one tick and drops the expired ones, keeping the rest in order.
    fn advance(&mut self) {
        for slot in &mut self.rows {
            *slot = slot
                .filter(|entry| entry.ticks_left > 1)
                .map(|entry| FeedEntry {
                    ticks_left: entry.ticks_left - 1,
                    ..entry
                });
        }
    }
}

impl Resource for KillFeed {
    fn hash_into(&self, hasher: &mut StateHasher) {
        for entry in self.rows.iter().flatten() {
            hasher.write_u64(entry.killer as u64);
            hasher.write_u64(entry.victim as u64);
            hasher.write_u32(entry.ticks_left);
        }
    }
}

/// The index of the spawn point farthest from every threat, as the minimum distance to any threat.
/// Ties go to the lowest index. With no threats every point is equally far, so the first wins.
#[must_use]
pub fn farthest_spawn(spawns: &[Vec3], threats: impl Iterator<Item = Vec3> + Clone) -> usize {
    let mut best = 0;
    let mut best_clearance = f32::NEG_INFINITY;
    for (index, spawn) in spawns.iter().enumerate() {
        let clearance = threats
            .clone()
            .map(|threat| spawn.distance_squared(threat))
            .fold(f32::INFINITY, f32::min);
        if clearance > best_clearance {
            best = index;
            best_clearance = clearance;
        }
    }
    best
}

/// Counts every downed actor down one tick and respawns those whose countdown ends.
///
/// # Errors
///
/// Returns a world error when a resource or an actor's components are missing, or a value the
/// kit or physics rejects.
pub fn respawn_system(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let actor_count = world.resource::<Actors>()?.slots().len();
    for slot in 0..actor_count {
        let actor = actor_at(world, slot)?;
        if let Life::Down { ticks_left } = actor.life {
            let left = ticks_left.saturating_sub(1);
            if left == 0 {
                respawn(world, slot)?;
            } else if let Some(entry) = world.resource_mut::<Actors>()?.slot_mut(slot) {
                entry.life = Life::Down { ticks_left: left };
            }
        }
    }
    Ok(())
}

/// Advances the match clock and the kill feed by one tick, then scores the tick's deaths.
///
/// # Errors
///
/// Returns a world error when a resource or an event log is missing.
pub fn rules_system(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let score = world.resource_mut::<Score>()?;
    score.ticks = score.ticks.saturating_add(1);
    world.resource_mut::<KillFeed>()?.advance();
    let deaths = world.events::<Killed>()?.len();
    for index in 0..deaths {
        let Some(kill) = world.events::<Killed>()?.get(index).copied() else {
            break;
        };
        record_death(world, kill)?;
    }
    Ok(())
}

/// Puts a living actor down, scores the death if it is new, and credits the kill to the actor
/// that caused it. A death caused by something that is not an actor (a lethal mover) scores no
/// kill.
fn record_death(world: &mut World, kill: Killed) -> Result<(), WorldError> {
    let Some(slot) = world.resource::<Actors>()?.slot_of(kill.victim) else {
        return Ok(());
    };
    if !actor_at(world, slot)?.is_alive() {
        return Ok(());
    }
    let respawn = respawn_ticks(world, slot)?;
    if let Some(entry) = world.resource_mut::<Actors>()?.slot_mut(slot) {
        entry.life = Life::Down {
            ticks_left: respawn,
        };
    }
    world
        .resource_mut::<Score>()?
        .tally_mut(slot)
        .ok_or(WorldError::Invariant("actor slot is out of range"))?
        .deaths += 1;
    let killer_slot = world.resource::<Actors>()?.slot_of(kill.cause);
    if let Some(killer_slot) = killer_slot.filter(|&k| k != slot) {
        world
            .resource_mut::<Score>()?
            .tally_mut(killer_slot)
            .ok_or(WorldError::Invariant("actor slot is out of range"))?
            .kills += 1;
        let ticks_left = ms_to_ticks(world.resource::<Tuning>()?.rules.kill_feed_ms).ok_or(
            WorldError::Invariant("kill feed duration overflows the tick count"),
        )?;
        world.resource_mut::<KillFeed>()?.push(FeedEntry {
            killer: killer_slot,
            victim: slot,
            ticks_left,
        });
    }
    Ok(())
}

/// Ticks a down actor waits before it respawns. The durations come from the world's tuning.
fn respawn_ticks(world: &World, slot: usize) -> Result<u32, WorldError> {
    let tuning = world.resource::<Tuning>()?;
    let actor = actor_at(world, slot)?;
    let ms = if world.get::<Bot>(actor.id).is_ok() {
        tuning.respawn.bot_ms
    } else {
        tuning.respawn.player_ms
    };
    ms_to_ticks(ms).ok_or(WorldError::Invariant(
        "respawn delay overflows the tick count",
    ))
}

/// Brings a down actor back: the player at the spawn farthest from the living bots, a bot at the
/// spawn farthest from the player. Restores health, ammunition and locomotion, resets a bot's
/// brain, and places the body.
fn respawn(world: &mut World, slot: usize) -> Result<(), WorldError> {
    let actor = actor_at(world, slot)?;
    let is_bot = world.get::<Bot>(actor.id).is_ok();
    let tuning = world.resource::<Tuning>()?.clone();
    let arena = world.resource::<ArenaData>()?;
    let (feet, max_health) = if !is_bot {
        let threats = living_bot_centres(world)?;
        let index = farthest_spawn(arena.player_spawns(), threats.into_iter());
        (
            spawn_point(arena.player_spawns(), index)?,
            tuning.actors.player_health,
        )
    } else {
        let player = actor_at(world, PLAYER_SLOT)?.id;
        let actor_count = world.resource::<Actors>()?.slots().len();
        let mut threats = Vec::with_capacity(actor_count);
        threats.push(world.get::<Locomotion>(player)?.centre());
        for other_slot in 0..world.resource::<Actors>()?.slots().len() {
            if other_slot == slot {
                continue;
            }
            let other = actor_at(world, other_slot)?;
            if other.is_alive() {
                threats.push(world.get::<Locomotion>(other.id)?.centre());
            }
        }
        let index = farthest_spawn(arena.bot_spawns(), threats.into_iter());
        (
            spawn_point(arena.bot_spawns(), index)?,
            tuning.actors.bot_health,
        )
    };
    let centre = feet + Vec3::Y * (tuning.movement.stand_height * 0.5);
    let collider = world.get::<Locomotion>(actor.id)?.collider();
    let stand_height = tuning.movement.stand_height;
    let shape = ConvexShape::capsule(stand_height * 0.5 - CAPSULE_RADIUS_M, CAPSULE_RADIUS_M)
        .map_err(downstream)?;
    world
        .resource_mut::<PhysicsWorld>()?
        .set_collider_shape(collider, shape)
        .map_err(downstream)?;
    world
        .resource_mut::<PhysicsWorld>()?
        .set_body_pose(actor.body, centre, Quat::IDENTITY)
        .map_err(downstream)?;
    *world.get_mut::<Locomotion>(actor.id)? = kit_fps::error::lift(Locomotion::new(
        actor.body,
        collider,
        centre,
        EYE_ABOVE_CENTRE_M,
    ))?;
    *world.get_mut::<Health>(actor.id)? = kit_fps::error::lift(Health::full(max_health))?;
    *world.get_mut::<Weapon>(actor.id)? = Weapon::new(tuning.weapon_config()?);
    if is_bot {
        let stream = (actor.stream << 32) | u64::from(actor.respawns);
        let brain_rng = world.rng().fork(stream);
        *world.get_mut::<Bot>(actor.id)? = Bot::new(tuning.bot_config()?, brain_rng);
    }
    let entry = world
        .resource_mut::<Actors>()?
        .slot_mut(slot)
        .ok_or(WorldError::Invariant("actor slot vanished during respawn"))?;
    entry.life = Life::Alive;
    entry.respawns = entry.respawns.saturating_add(1);
    Ok(())
}

/// The actor in `slot`, or an invariant error when the slot does not exist.
fn actor_at(world: &World, slot: usize) -> Result<Actor, WorldError> {
    world
        .resource::<Actors>()?
        .get(slot)
        .ok_or(WorldError::Invariant("actor slot is out of range"))
}

/// The spawn point at `index`, or an invariant error when the index is out of range.
fn spawn_point(points: &[Vec3], index: usize) -> Result<Vec3, WorldError> {
    points
        .get(index)
        .copied()
        .ok_or(WorldError::Invariant("spawn index is out of range"))
}

/// The centres of the living bots, in a fixed buffer, and how many there are. No allocation.
fn living_bot_centres(world: &World) -> Result<Vec<Vec3>, WorldError> {
    let actor_count = world.resource::<Actors>()?.slots().len();
    let mut buffer = Vec::with_capacity(actor_count.saturating_sub(1));
    for slot in (PLAYER_SLOT + 1)..actor_count {
        let actor = actor_at(world, slot)?;
        if !actor.is_alive() || world.get::<Bot>(actor.id).is_err() {
            continue;
        }
        let centre = world.get::<Locomotion>(actor.id)?.centre();
        buffer.push(centre);
    }
    Ok(buffer)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn point(x: f32, z: f32) -> Vec3 {
        Vec3::new(x, 0.0, z)
    }

    #[test]
    fn with_no_threats_the_first_spawn_wins() {
        let spawns = [point(0.0, 0.0), point(5.0, 0.0)];
        assert_eq!(farthest_spawn(&spawns, std::iter::empty()), 0);
    }

    #[test]
    fn a_tie_goes_to_the_lowest_index() {
        // Both spawns are 5 m from the single threat at the origin.
        let spawns = [point(5.0, 0.0), point(-5.0, 0.0)];
        assert_eq!(farthest_spawn(&spawns, std::iter::once(point(0.0, 0.0))), 0);
    }

    #[test]
    fn the_farthest_spawn_from_the_threat_wins() {
        let spawns = [point(1.0, 0.0), point(9.0, 0.0), point(4.0, 0.0)];
        assert_eq!(farthest_spawn(&spawns, std::iter::once(point(0.0, 0.0))), 1);
    }

    #[test]
    fn an_actor_list_without_a_player_is_rejected() {
        assert!(Actors::new(Vec::new()).is_err());
    }

    proptest! {
        #[test]
        fn the_chosen_spawn_has_the_largest_clearance(
            spawns in proptest::collection::vec((-30.0f32..30.0, -30.0f32..30.0), 1..8),
            threats in proptest::collection::vec((-30.0f32..30.0, -30.0f32..30.0), 0..6),
        ) {
            let spawns: Vec<Vec3> = spawns.into_iter().map(|(x, z)| point(x, z)).collect();
            let threats: Vec<Vec3> = threats.into_iter().map(|(x, z)| point(x, z)).collect();
            let clearance = |spawn: Vec3| {
                threats
                    .iter()
                    .map(|t| spawn.distance_squared(*t))
                    .fold(f32::INFINITY, f32::min)
            };
            let chosen = farthest_spawn(&spawns, threats.iter().copied());
            let best = spawns.iter().map(|s| clearance(*s)).fold(f32::NEG_INFINITY, f32::max);
            prop_assert!(clearance(spawns[chosen]) >= best);
        }
    }
}
