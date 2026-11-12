//! Perception: the [`Senses`] trait answers line-of-sight questions, [`PhysicsSenses`] answers them
//! from the physics world, [`sees`] applies a bot's sight range and field of view, and
//! [`pick_enemy`] chooses the enemy a bot turns on.
//!
//! Invariants: a sight query ignores the observer's own body; a segment shorter than a millimetre
//! counts as clear or seen, rather than producing an unstable direction.

use engine_physics::{BodyId, PhysicsWorld, QueryFilter, RayCast};
use engine_world::{NetId, World, WorldError};
use glam::Vec3;

use super::brain::BotInput;
use super::config::BotConfig;
use crate::command::look_direction;
use crate::controller::Locomotion;
use crate::error::{KitError, lift};
use crate::health::{Team, is_alive};

/// Treat sub-millimetre sight segments as contact instead of normalising unstable directions.
const SENSE_CONTACT_EPSILON_M: f32 = 0.001;

/// An enemy as a bot perceives it: where its eye is and which body it moves with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// Eye position in world space.
    pub eye: Vec3,
    /// The kinematic body the enemy moves with, so a ray can recognise it.
    pub body: BodyId,
}

/// Line-of-sight queries. The brain asks; the implementation answers from the world.
pub trait Senses {
    /// Whether a ray from `eye` towards `target_eye` reaches `target`'s body before any other body.
    /// `observer` is the bot's own body and is ignored.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] when the query cannot be built or the physics world rejects it.
    fn can_see(
        &self,
        observer: BodyId,
        eye: Vec3,
        target: BodyId,
        target_eye: Vec3,
    ) -> Result<bool, KitError>;

    /// Whether any body other than `observer` lies within `range` metres of `origin` along
    /// `direction`.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] when the query cannot be built or the physics world rejects it.
    fn blocked(
        &self,
        observer: BodyId,
        origin: Vec3,
        direction: Vec3,
        range: f32,
    ) -> Result<bool, KitError>;

    /// Whether the straight segment from `from` to `to` meets no body. When `observer` is given,
    /// that body is ignored. Waypoint links and routing use this to ask whether a route is open.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] when the query cannot be built or the physics world rejects it.
    fn clear(&self, observer: Option<BodyId>, from: Vec3, to: Vec3) -> Result<bool, KitError>;
}

/// [`Senses`] answered by rays through the physics world.
pub struct PhysicsSenses<'a> {
    physics: &'a PhysicsWorld,
    filter: QueryFilter,
}

impl<'a> PhysicsSenses<'a> {
    /// Senses that read the given physics world.
    #[must_use]
    pub const fn new(physics: &'a PhysicsWorld) -> Self {
        Self {
            physics,
            filter: QueryFilter {
                layers: engine_physics::LayerMask::ALL,
                exclude_body: None,
                include_sensors: false,
            },
        }
    }

    /// Senses that use a game-selected collider filter for static navigation queries.
    #[must_use]
    pub const fn with_filter(physics: &'a PhysicsWorld, filter: QueryFilter) -> Self {
        Self { physics, filter }
    }
}

impl Senses for PhysicsSenses<'_> {
    fn can_see(
        &self,
        observer: BodyId,
        eye: Vec3,
        target: BodyId,
        target_eye: Vec3,
    ) -> Result<bool, KitError> {
        let offset = target_eye - eye;
        let distance = offset.length();
        if distance <= SENSE_CONTACT_EPSILON_M {
            return Ok(true);
        }
        let request = RayCast::new(eye, offset, distance, true)?;
        let hit = self
            .physics
            .ray_cast(request, self.filter.excluding_body(observer));
        Ok(hit.is_some_and(|hit| hit.owner.body == Some(target)))
    }

    fn blocked(
        &self,
        observer: BodyId,
        origin: Vec3,
        direction: Vec3,
        range: f32,
    ) -> Result<bool, KitError> {
        let request = RayCast::new(origin, direction, range, true)?;
        Ok(self
            .physics
            .ray_cast(request, self.filter.excluding_body(observer))
            .is_some())
    }

    fn clear(&self, observer: Option<BodyId>, from: Vec3, to: Vec3) -> Result<bool, KitError> {
        let offset = to - from;
        let distance = offset.length();
        if distance <= SENSE_CONTACT_EPSILON_M {
            return Ok(true);
        }
        let request = RayCast::new(from, offset, distance, true)?;
        let hit = match observer {
            Some(body) => self
                .physics
                .ray_cast(request, self.filter.excluding_body(body)),
            None => self.physics.ray_cast(request, self.filter),
        };
        Ok(hit.is_none())
    }
}

/// Whether `target` is visible from the bot: within sight range and field of view, and clear of
/// any body but the target's along the line of sight.
pub(super) fn sees<S: Senses>(
    cfg: &BotConfig,
    input: &BotInput,
    target: Target,
    offset: Vec3,
    senses: &S,
) -> Result<bool, KitError> {
    let distance = offset.length();
    if distance > cfg.sight_range {
        return Ok(false);
    }
    if distance > 0.0 && look_direction(input.view).angle_between(offset) > cfg.fov_rad * 0.5 {
        return Ok(false);
    }
    senses.can_see(input.body, input.eye, target.body, target.eye)
}

/// The enemy a bot turns on: the nearest living enemy it has a clear line to, or, when none is
/// in the open, the nearest living enemy so it can still chase. Several enemies (a free-for-all)
/// make the difference: a bot fights the one it can shoot, not one behind a wall. Scans the world
/// without allocating; ties keep the entity found first, in ascending id order.
pub(super) fn pick_enemy<S: Senses>(
    world: &World,
    me: NetId,
    team: Team,
    locomotion: &Locomotion,
    senses: &S,
) -> Result<Option<Target>, WorldError> {
    let origin = locomotion.eye();
    let mut nearest: Option<(f32, Target)> = None;
    let mut nearest_in_sight: Option<(f32, Target)> = None;
    for id in world.net_ids() {
        if id == me {
            continue;
        }
        let Ok(their_team) = world.get::<Team>(id) else {
            continue;
        };
        if *their_team == team {
            continue;
        }
        let Ok(theirs) = world.get::<Locomotion>(id) else {
            continue;
        };
        if !is_alive(world, id)? {
            continue;
        }
        let distance_squared = (theirs.eye() - origin).length_squared();
        let target = Target {
            eye: theirs.eye(),
            body: theirs.body(),
        };
        if nearest.is_none_or(|(closest, _)| distance_squared < closest) {
            nearest = Some((distance_squared, target));
        }
        let closer = nearest_in_sight.is_none_or(|(closest, _)| distance_squared < closest);
        if closer && lift(senses.can_see(locomotion.body(), origin, target.body, target.eye))? {
            nearest_in_sight = Some((distance_squared, target));
        }
    }
    Ok(nearest_in_sight.or(nearest).map(|(_, target)| target))
}
