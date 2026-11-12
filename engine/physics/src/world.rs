//! Ownership boundary for Rapier state, fixed stepping, typed queries, and stable hashing.
//!
//! Bodies hash in Rapier arena order, which is ascending `(slot, generation)` order.
//! Each record hashes a tag, its identifier, translation, quaternion, linear velocity,
//! and angular velocity in that exact order; a closing tag ends the sequence.

use crate::{
    BodyBuilder, BodyId, BodyKind, BodyState, ColliderBuilder, ColliderId, ColliderOwner,
    ColliderShape, ConvexShape, PhysicsConfig, PhysicsError, QueryFilter, RayCast, RayCastHit,
    ShapeCast, ShapeCastHit,
    convert::{from_rapier_pose, from_rapier_vec, to_rapier_pose, to_rapier_vec},
};
use engine_core::StateHasher;
use engine_world::Resource;
use glam::{Quat, Vec3};
use rapier3d::parry::query::{Ray, ShapeCastOptions};
use rapier3d::prelude::QueryFilter as RapierQueryFilter;
use rapier3d::prelude::*;

/// A self-contained deterministic fixed-step physics simulation.
pub struct PhysicsWorld {
    config: PhysicsConfig,
    pipeline: PhysicsPipeline,
    integration_parameters: IntegrationParameters,
    islands: IslandManager,
    broad_phase: BroadPhaseBvh,
    narrow_phase: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    soft_bodies: SoftBodySet,
    ccd_solver: CCDSolver,
    broad_phase_pair_events: Vec<BroadPhasePairEvent>,
}

impl PhysicsWorld {
    /// Creates an empty world with an explicit fixed timestep and gravity.
    #[must_use]
    pub fn new(config: PhysicsConfig) -> Self {
        let integration_parameters = IntegrationParameters {
            dt: config.dt(),
            ..IntegrationParameters::default()
        };
        Self {
            config,
            pipeline: PhysicsPipeline::new(),
            integration_parameters,
            islands: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            soft_bodies: SoftBodySet::new(),
            ccd_solver: CCDSolver::new(),
            broad_phase_pair_events: Vec::with_capacity(8),
        }
    }

    /// Returns the immutable fixed-step configuration.
    #[must_use]
    pub const fn config(&self) -> PhysicsConfig {
        self.config
    }

    /// Returns the number of live rigid bodies.
    #[must_use]
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    /// Returns the number of live colliders.
    #[must_use]
    pub fn collider_count(&self) -> usize {
        self.colliders.len()
    }

    /// Inserts one validated rigid body.
    pub fn insert_body(&mut self, builder: BodyBuilder) -> BodyId {
        BodyId::from_handle(self.bodies.insert(builder.build()))
    }

    /// Inserts one validated collider, optionally attached to a live body.
    ///
    /// # Errors
    /// Returns [`PhysicsError::StaleBody`] when `parent` does not name a live body.
    pub fn insert_collider(
        &mut self,
        parent: Option<BodyId>,
        builder: ColliderBuilder,
    ) -> Result<ColliderId, PhysicsError> {
        let handle = if let Some(parent_id) = parent {
            let Some(body) = self.bodies.get(parent_id.handle()) else {
                return Err(PhysicsError::StaleBody(parent_id));
            };
            if body.is_dynamic() && builder.shape().is_trimesh() {
                return Err(PhysicsError::DynamicTriangleMesh);
            }
            let collider = builder.build();
            self.colliders
                .insert_with_parent(collider, parent_id.handle(), &mut self.bodies)
        } else {
            let collider = builder.build();
            self.colliders.insert(collider)
        };
        self.sync_collider_in_broad_phase(handle);
        Ok(ColliderId::from_handle(handle))
    }

    /// Replaces a collider's validated shape. When the collider is attached to a body, the body
    /// moves so the collider's lowest point stays where it was: a character's feet stay put as
    /// its capsule height changes. A free collider changes shape in place.
    ///
    /// # Errors
    /// Returns [`PhysicsError::StaleCollider`] when `id` is not live, or
    /// [`PhysicsError::StaleBody`] when its parent is stale.
    pub fn set_collider_shape(
        &mut self,
        id: ColliderId,
        shape: impl Into<ColliderShape>,
    ) -> Result<(), PhysicsError> {
        let shape = shape.into();
        let parent = self
            .colliders
            .get(id.handle())
            .ok_or(PhysicsError::StaleCollider(id))?
            .parent();
        if shape.is_trimesh()
            && parent
                .and_then(|handle| self.bodies.get(handle))
                .is_some_and(RigidBody::is_dynamic)
        {
            return Err(PhysicsError::DynamicTriangleMesh);
        }
        let collider = self
            .colliders
            .get_mut(id.handle())
            .ok_or(PhysicsError::StaleCollider(id))?;
        let old_min_y = collider.shape().compute_local_aabb().mins.y;
        collider.set_shape(shape.shared());
        let new_min_y = collider.shape().compute_local_aabb().mins.y;
        // The local offset to the parent does not change, so the lowest point moves by the
        // difference of the shapes' own lower extents.
        if let Some(parent) = collider.parent() {
            let body_id = BodyId::from_handle(parent);
            let body = self
                .bodies
                .get_mut(parent)
                .ok_or(PhysicsError::StaleBody(body_id))?;
            let mut translation = body.translation();
            translation.y += old_min_y - new_min_y;
            body.set_translation(translation, true);
        }
        self.sync_collider_in_broad_phase(id.handle());
        Ok(())
    }

    /// Removes a collider and invalidates its identifier.
    ///
    /// # Errors
    /// Returns [`PhysicsError::StaleCollider`] when `id` is not live.
    pub fn remove_collider(&mut self, id: ColliderId) -> Result<(), PhysicsError> {
        let removed = self.colliders.remove(
            id.handle(),
            &mut self.islands,
            &mut self.bodies,
            &mut self.soft_bodies,
            true,
        );
        if removed.is_none() {
            return Err(PhysicsError::StaleCollider(id));
        }
        self.remove_collider_from_broad_phase(id.handle());
        Ok(())
    }

    /// Removes a body, its attached colliders, and attached joints.
    ///
    /// # Errors
    /// Returns [`PhysicsError::StaleBody`] when `id` is not live.
    pub fn remove_body(&mut self, id: BodyId) -> Result<(), PhysicsError> {
        let body = self
            .bodies
            .get(id.handle())
            .ok_or(PhysicsError::StaleBody(id))?;
        let attached_colliders: Vec<ColliderHandle> = body.colliders().to_vec();
        let removed = self.bodies.remove(
            id.handle(),
            &mut self.islands,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            true,
        );
        if removed.is_none() {
            return Err(PhysicsError::StaleBody(id));
        }
        for handle in attached_colliders {
            self.remove_collider_from_broad_phase(handle);
        }
        Ok(())
    }

    /// Reads a body's current pose and velocities.
    ///
    /// # Errors
    /// Returns [`PhysicsError::StaleBody`] when `id` is not live.
    pub fn body_state(&self, id: BodyId) -> Result<BodyState, PhysicsError> {
        let body = self
            .bodies
            .get(id.handle())
            .ok_or(PhysicsError::StaleBody(id))?;
        Ok(body_state(body))
    }

    /// Sets a body's current pose and wakes it.
    ///
    /// Attached collider queries observe the pose after the next fixed step.
    ///
    /// # Errors
    /// Returns an error for an invalid pose or stale body identifier.
    pub fn set_body_pose(
        &mut self,
        id: BodyId,
        translation: Vec3,
        rotation: Quat,
    ) -> Result<(), PhysicsError> {
        crate::config::validate_vec3(translation, "body translation")?;
        crate::config::validate_quat(rotation, "body rotation")?;
        let body = self
            .bodies
            .get_mut(id.handle())
            .ok_or(PhysicsError::StaleBody(id))?;
        body.set_position(to_rapier_pose(translation, rotation.normalize()), true);
        Ok(())
    }

    /// Sets the target pose used by a position-based kinematic body on the next step.
    ///
    /// # Errors
    /// Returns an error for an invalid pose, stale body identifier, or non-kinematic body.
    pub fn set_next_kinematic_pose(
        &mut self,
        id: BodyId,
        translation: Vec3,
        rotation: Quat,
    ) -> Result<(), PhysicsError> {
        crate::config::validate_vec3(translation, "kinematic translation")?;
        crate::config::validate_quat(rotation, "kinematic rotation")?;
        let body = self
            .bodies
            .get_mut(id.handle())
            .ok_or(PhysicsError::StaleBody(id))?;
        if !body.is_kinematic() {
            return Err(PhysicsError::WrongBodyKind {
                body: id,
                required: BodyKind::Kinematic,
            });
        }
        body.set_next_kinematic_position(to_rapier_pose(translation, rotation.normalize()));
        Ok(())
    }

    /// Advances the simulation by exactly [`PhysicsConfig::dt`] seconds.
    ///
    /// The method reuses all wrapper-owned state. Rapier may grow internal arenas
    /// when topology or contact complexity exceeds previously warmed capacity.
    pub fn step(&mut self) {
        self.pipeline.step(
            to_rapier_vec(self.config.gravity()),
            &self.integration_parameters,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.soft_bodies,
            &mut self.ccd_solver,
            &(),
            &(),
        );
    }

    /// Casts a validated ray and returns the closest typed hit.
    #[must_use]
    pub fn ray_cast(&self, request: RayCast, filter: QueryFilter) -> Option<RayCastHit> {
        self.ray_cast_with_filter(filter, request)
    }

    /// Shared body of the ray casts: casts `request` against the colliders `filter` accepts.
    fn ray_cast_with_filter(&self, filter: QueryFilter, request: RayCast) -> Option<RayCastHit> {
        let predicate = |_: ColliderHandle, collider: &Collider| filter.accepts(collider);
        let query_filter = filter.rapier_filter(&predicate);
        let ray = Ray::new(
            to_rapier_vec(request.origin()),
            to_rapier_vec(request.direction()),
        );
        let queries = self.query_pipeline(query_filter);
        queries
            .cast_ray_and_get_normal(&ray, request.max_distance(), request.solid())
            .map(|(handle, hit)| RayCastHit {
                owner: self.owner(handle),
                distance: hit.time_of_impact,
                point: from_rapier_vec(ray.point_at(hit.time_of_impact)),
                normal: from_rapier_vec(hit.normal),
                surface: self.surface(handle),
            })
    }

    /// Sweeps a validated convex shape and returns the closest typed hit.
    #[must_use]
    pub fn shape_cast(&self, request: ShapeCast, filter: QueryFilter) -> Option<ShapeCastHit> {
        let shape = request.shape().shared();
        let options = ShapeCastOptions {
            max_time_of_impact: 1.0,
            stop_at_penetration: request.stop_at_penetration_value(),
            ..ShapeCastOptions::default()
        };
        let predicate = |_: ColliderHandle, collider: &Collider| filter.accepts(collider);
        let query_filter = filter.rapier_filter(&predicate);
        let queries = self.query_pipeline(query_filter);
        queries
            .cast_shape(
                &to_rapier_pose(request.translation(), request.rotation()),
                to_rapier_vec(request.velocity()),
                shape.as_ref(),
                options,
            )
            .map(|(handle, hit)| ShapeCastHit {
                owner: self.owner(handle),
                distance: hit.time_of_impact * request.velocity().length(),
                surface: self.surface(handle),
            })
    }

    /// Returns whether a shape at a pose overlaps any collider accepted by `filter`.
    ///
    /// # Errors
    /// Returns an error for invalid pose data.
    pub fn intersects_shape(
        &self,
        shape: ConvexShape,
        translation: Vec3,
        rotation: Quat,
        filter: QueryFilter,
    ) -> Result<bool, PhysicsError> {
        self.intersects_with_filter(filter, shape, translation, rotation)
    }

    /// Shared body of the intersection queries: validates the pose and tests the shape against
    /// the colliders that `filter` accepts.
    fn intersects_with_filter(
        &self,
        filter: QueryFilter,
        shape: ConvexShape,
        translation: Vec3,
        rotation: Quat,
    ) -> Result<bool, PhysicsError> {
        crate::config::validate_vec3(translation, "intersection translation")?;
        crate::config::validate_quat(rotation, "intersection rotation")?;
        let shared = shape.shared();
        let predicate = |_: ColliderHandle, collider: &Collider| filter.accepts(collider);
        let query_filter = filter.rapier_filter(&predicate);
        let queries = self.query_pipeline(query_filter);
        Ok(queries
            .intersect_shape(
                to_rapier_pose(translation, rotation.normalize()),
                shared.as_ref(),
            )
            .next()
            .is_some())
    }

    /// Hashes body transforms and velocities using [`StateHasher`].
    ///
    /// Records are emitted in Rapier arena order. Each record starts with a `1`
    /// tag (so the body boundary is hashed), then carries the full
    /// generation-checked identifier, translation `x/y/z`, rotation `x/y/z/w`,
    /// linear velocity `x/y/z`, then angular velocity `x/y/z`. A `0` tag ends the
    /// sequence; an empty world hashes to the digest of that single tag.
    #[must_use]
    pub fn state_hash(&self) -> u64 {
        let mut hasher = StateHasher::new();
        for (handle, body) in self.bodies.iter() {
            hasher.write_u8(1);
            let id = BodyId::from_handle(handle);
            let state = body_state(body);
            hasher.write_u32(id.index());
            hasher.write_u32(id.generation());
            write_vec3(&mut hasher, state.translation);
            hasher.write_f32(state.rotation.x);
            hasher.write_f32(state.rotation.y);
            hasher.write_f32(state.rotation.z);
            hasher.write_f32(state.rotation.w);
            write_vec3(&mut hasher, state.linear_velocity);
            write_vec3(&mut hasher, state.angular_velocity);
        }
        hasher.write_u8(0);
        hasher.finish()
    }

    pub(crate) fn query_pipeline<'a>(&'a self, filter: RapierQueryFilter<'a>) -> QueryPipeline<'a> {
        self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            filter,
        )
    }

    fn sync_collider_in_broad_phase(&mut self, handle: ColliderHandle) {
        self.broad_phase_pair_events.clear();
        self.broad_phase.update(
            &self.integration_parameters,
            &self.colliders,
            &self.bodies,
            &[handle],
            &[],
            &mut self.broad_phase_pair_events,
        );
    }

    fn remove_collider_from_broad_phase(&mut self, handle: ColliderHandle) {
        self.broad_phase_pair_events.clear();
        self.broad_phase.update(
            &self.integration_parameters,
            &self.colliders,
            &self.bodies,
            &[],
            &[handle],
            &mut self.broad_phase_pair_events,
        );
    }

    fn owner(&self, handle: ColliderHandle) -> ColliderOwner {
        let body = self
            .colliders
            .get(handle)
            .and_then(|collider| collider.parent())
            .map(BodyId::from_handle);
        ColliderOwner {
            collider: ColliderId::from_handle(handle),
            body,
        }
    }

    fn surface(&self, handle: ColliderHandle) -> crate::SurfaceId {
        self.colliders
            .get(handle)
            .map_or(crate::SurfaceId::DEFAULT, |collider| {
                crate::SurfaceId(u16::try_from(collider.user_data).unwrap_or(0))
            })
    }
}

/// The physics world can live inside an `engine-world` `World` as a resource. Its
/// hash enters the world's state hash through [`PhysicsWorld::state_hash`].
impl Resource for PhysicsWorld {
    fn hash_into(&self, hasher: &mut StateHasher) {
        hasher.write_u64(self.state_hash());
    }
}

fn body_state(body: &RigidBody) -> BodyState {
    let (translation, rotation) = from_rapier_pose(body.position());
    BodyState {
        translation,
        rotation,
        linear_velocity: from_rapier_vec(body.linvel()),
        angular_velocity: from_rapier_vec(body.angvel()),
        sleeping: body.is_sleeping(),
    }
}

fn write_vec3(hasher: &mut StateHasher, value: Vec3) {
    hasher.write_f32(value.x);
    hasher.write_f32(value.y);
    hasher.write_f32(value.z);
}
