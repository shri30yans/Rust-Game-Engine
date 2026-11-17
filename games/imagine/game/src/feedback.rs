//! Feedback for the HUD and the presenter: hit markers, kill flashes, damage directions and the
//! tracers of recent shots. [`capture_feedback`] reads the tick's kit events ([`ShotFired`],
//! [`Damaged`], [`Killed`]) after the damage pass; it is one of several readers of those events and
//! may run in any order among them.
//!
//! Invariants: every timer counts down once per tick and never goes below zero; a tracer is
//! recorded only for a shot the weapon fired; the tracer follows the aim line, not the sampled
//! pellet, because the kit does not expose pellet directions; this system writes only
//! [`HudEvents`].

use engine_core::StateHasher;
use engine_physics::{PhysicsWorld, RayCast};
use engine_world::{NetId, Resource, SystemContext, World, WorldError};
use glam::Vec3;
use kit_fps::{Damaged, Killed, Locomotion, ShotFired};

use crate::data::{FeedbackTuning, Tuning};
use crate::error::downstream;
use crate::rules::{Actors, PLAYER_SLOT};

/// How many damage direction indicators can show at once.
pub const HURT_SLOTS: usize = 4;

/// A damage direction indicator: where a hit came from, relative to the player's view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hurt {
    bearing_rad: f32,
    ticks_left: u32,
}

impl Hurt {
    /// The shooter's bearing from the player's view, in radians. Positive is to the right.
    #[must_use]
    pub const fn bearing_rad(self) -> f32 {
        self.bearing_rad
    }

    /// Ticks the indicator stays on screen.
    #[must_use]
    pub const fn ticks_left(self) -> u32 {
        self.ticks_left
    }
}

/// The tracer of one shot: from the eye along the aim line to the first hit, or to the full range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracer {
    from: Vec3,
    to: Vec3,
    age: u32,
}

impl Tracer {
    /// Where the shot left the shooter's eye.
    #[must_use]
    pub const fn from(self) -> Vec3 {
        self.from
    }

    /// Where the shot ended.
    #[must_use]
    pub const fn to(self) -> Vec3 {
        self.to
    }

    /// Ticks since the shot was fired.
    #[must_use]
    pub const fn age(self) -> u32 {
        self.age
    }

    /// Whether the muzzle flash should still show for this shot.
    #[must_use]
    pub fn is_muzzle_flash(self, tuning: &FeedbackTuning) -> bool {
        self.age < tuning.muzzle_flash_ticks
    }
}

/// The feedback the HUD and the presenter read. A resource.
pub struct HudEvents {
    hit_marker_ticks: u32,
    kill_flash_ticks: u32,
    hurt: [Option<Hurt>; HURT_SLOTS],
    tracers: Vec<Option<Tracer>>,
    shots_fired: Vec<u32>,
    player_hits: u32,
    player_kills: u32,
}

impl HudEvents {
    /// Empty feedback with one tracer and shot counter for each actor slot.
    #[must_use]
    pub fn new(actor_count: usize) -> Self {
        Self {
            hit_marker_ticks: 0,
            kill_flash_ticks: 0,
            hurt: [None; HURT_SLOTS],
            tracers: vec![None; actor_count],
            shots_fired: vec![0; actor_count],
            player_hits: 0,
            player_kills: 0,
        }
    }

    /// Ticks the player's hit marker stays on screen (zero when none).
    #[must_use]
    pub const fn hit_marker_ticks(&self) -> u32 {
        self.hit_marker_ticks
    }

    /// Ticks the kill flash stays on screen (zero when none).
    #[must_use]
    pub const fn kill_flash_ticks(&self) -> u32 {
        self.kill_flash_ticks
    }

    /// The damage direction indicators; each slot is `None` when empty.
    #[must_use]
    pub fn hurt(&self) -> &[Option<Hurt>] {
        &self.hurt
    }

    /// The latest tracer of each actor slot; `None` when that actor's shot has faded.
    #[must_use]
    pub fn tracers(&self) -> &[Option<Tracer>] {
        &self.tracers
    }

    /// Monotonic shot counters, one for each actor slot.
    pub(crate) fn shots_fired(&self) -> &[u32] {
        &self.shots_fired
    }

    /// Number of hit events credited to the local player.
    pub(crate) const fn player_hits(&self) -> u32 {
        self.player_hits
    }

    /// Number of kill events credited to the local player.
    pub(crate) const fn player_kills(&self) -> u32 {
        self.player_kills
    }

    /// Starts the kill flash and counts a kill credited to the local player, for audio playback.
    fn record_player_kill(&mut self, tuning: &FeedbackTuning) {
        self.kill_flash_ticks = tuning.kill_flash_ticks;
        self.player_kills = self.player_kills.wrapping_add(1);
    }

    /// Counts every timer and tracer down one tick.
    fn advance(&mut self, tuning: &FeedbackTuning) {
        self.hit_marker_ticks = self.hit_marker_ticks.saturating_sub(1);
        self.kill_flash_ticks = self.kill_flash_ticks.saturating_sub(1);
        for slot in &mut self.hurt {
            *slot = slot.filter(|hurt| hurt.ticks_left > 1).map(|hurt| Hurt {
                ticks_left: hurt.ticks_left - 1,
                ..hurt
            });
        }
        for slot in &mut self.tracers {
            *slot = slot
                .map(|tracer| Tracer {
                    age: tracer.age.saturating_add(1),
                    ..tracer
                })
                .filter(|tracer| tracer.age < tuning.tracer_ticks);
        }
    }

    /// Shows a damage direction in the emptiest indicator, or the one closest to expiring.
    fn show_hurt(&mut self, bearing_rad: f32, tuning: &FeedbackTuning) {
        let emptiest = self
            .hurt
            .iter()
            .enumerate()
            .min_by_key(|(_, slot)| slot.map_or(0, |hurt| hurt.ticks_left))
            .map(|(index, _)| index);
        if let Some(entry) = emptiest.and_then(|index| self.hurt.get_mut(index)) {
            *entry = Some(Hurt {
                bearing_rad,
                ticks_left: tuning.damage_indicator_ticks,
            });
        }
    }
}

impl Resource for HudEvents {
    fn hash_into(&self, hasher: &mut StateHasher) {
        hasher.write_u32(self.hit_marker_ticks);
        hasher.write_u32(self.kill_flash_ticks);
        for slot in &self.hurt {
            hasher.write_u32(slot.map_or(0, |hurt| hurt.ticks_left));
        }
        hasher.write_u32(self.player_hits);
        hasher.write_u32(self.player_kills);
        for (slot, shots) in self.tracers.iter().zip(&self.shots_fired) {
            hasher.write_u32(*shots);
            if let Some(tracer) = slot {
                hasher.write_u32(tracer.age);
                hasher.write_f32(tracer.from.x);
                hasher.write_f32(tracer.to.x);
            }
        }
    }
}

/// Turns this tick's kit events into HUD feedback: a hit marker and a damage direction for each
/// applied hit, a tracer for each shot, and the kill flash for each kill the player made. Advances
/// the timers first.
///
/// # Errors
///
/// Returns a world error when a resource, an event log or an actor's components are missing, or
/// the physics world rejects a ray.
pub fn capture_feedback(world: &mut World, _ctx: &mut SystemContext<'_>) -> Result<(), WorldError> {
    let player = world
        .resource::<Actors>()?
        .get(PLAYER_SLOT)
        .ok_or(WorldError::Invariant("the player actor is missing"))?
        .id();
    let feedback = world.resource::<Tuning>()?.feedback;
    world.resource_mut::<HudEvents>()?.advance(&feedback);

    let count = world.events::<Damaged>()?.len();
    for index in 0..count {
        let Some(damaged) = world.events::<Damaged>()?.get(index).copied() else {
            break;
        };
        record_hit(world, player, damaged, &feedback)?;
    }
    let count = world.events::<ShotFired>()?.len();
    for index in 0..count {
        let Some(shot) = world.events::<ShotFired>()?.get(index).copied() else {
            break;
        };
        record_shot(world, shot)?;
    }
    let player_kills = world
        .events::<Killed>()?
        .iter()
        .filter(|kill| kill.cause == player && kill.victim != player)
        .count();
    let hud = world.resource_mut::<HudEvents>()?;
    for _ in 0..player_kills {
        hud.record_player_kill(&feedback);
    }
    Ok(())
}

/// Adds one applied hit to the feedback: a hit marker for the player's hit on another, and a damage
/// direction for another's hit on the player, pointing at where the hit came from.
fn record_hit(
    world: &mut World,
    player: NetId,
    damaged: Damaged,
    tuning: &FeedbackTuning,
) -> Result<(), WorldError> {
    let hit = damaged.hit;
    if hit.source() == player && hit.target() != player {
        let events = world.resource_mut::<HudEvents>()?;
        events.hit_marker_ticks = tuning.hit_marker_ticks;
        events.player_hits = events.player_hits.wrapping_add(1);
    }
    if hit.target() == player && hit.source() != player {
        let viewer = *world.get::<Locomotion>(player)?;
        let bearing = bearing_rad(&viewer, hit.origin());
        world
            .resource_mut::<HudEvents>()?
            .show_hurt(bearing, tuning);
    }
    Ok(())
}

/// Records the tracer of one shot for its shooter's slot. The tracer runs along the shooter's aim
/// from the eye to the first hit, or to the weapon's full range.
fn record_shot(world: &mut World, shot: ShotFired) -> Result<(), WorldError> {
    let Some(slot) = world.resource::<Actors>()?.slot_of(shot.shooter) else {
        return Ok(());
    };
    let events = world.resource_mut::<HudEvents>()?;
    let shot_count = events
        .shots_fired
        .get_mut(slot)
        .ok_or(WorldError::Invariant("shot counter slot is out of range"))?;
    *shot_count = shot_count.wrapping_add(1);
    let body = world.get::<Locomotion>(shot.shooter)?.body();
    let request = RayCast::new(shot.origin, shot.aim, shot.range_m, true).map_err(downstream)?;
    let hit = world.resource::<PhysicsWorld>()?.ray_cast(
        request,
        crate::collision::projectile_query().excluding_body(body),
    );
    let to = match hit {
        Some(hit) => hit.point,
        None => shot.origin + shot.aim * shot.range_m,
    };
    if let Some(entry) = world.resource_mut::<HudEvents>()?.tracers.get_mut(slot) {
        *entry = Some(Tracer {
            from: shot.origin,
            to,
            age: 0,
        });
    }
    Ok(())
}

/// The bearing of `target` from a viewer's centre, relative to the viewer's yaw. Positive is to the
/// right of the view.
fn bearing_rad(viewer: &Locomotion, target: Vec3) -> f32 {
    let yaw = viewer.command().view().yaw();
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let offset = target - viewer.centre();
    offset.dot(right).atan2(offset.dot(forward))
}
