//! Primitive shape registration, geometry, and physics spawning.
use engine_physics::{
    BodyBuilder, BodyId, ColliderBuilder, ConvexShape, PhysicsError, PhysicsWorld,
};
use engine_world::{NetId, Replication, Transform, World};
use glam::{EulerRot, Quat, Vec3};

use crate::error::SceneError;
use crate::model::{BodyDef, ShapeDef, ShapeKind};
use crate::visual::{MeshKind, Visual};

use super::{ColliderMetadata, SpawnedBody};

/// Registers the scene's components. Call once per world, before any spawn.
///
/// # Errors
///
/// Returns a world error when the components are already registered.
pub fn register_components(world: &mut World) -> Result<(), SceneError> {
    world.register::<Visual>(Replication::Replicated)?;
    Ok(())
}

/// The rotation of a shape: yaw, pitch and roll in degrees, then the ramp tilt about Z.
pub(super) fn rotation_of(shape: &ShapeDef) -> Quat {
    let [yaw, pitch, roll] = shape.rotation_deg().map(f32::to_radians);
    let base = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
    match shape.kind() {
        ShapeKind::Ramp { tilt_deg, .. } => base * Quat::from_rotation_z(tilt_deg.to_radians()),
        ShapeKind::Cuboid { .. } | ShapeKind::Sphere { .. } | ShapeKind::Capsule { .. } => base,
    }
}

fn collider_shape(kind: ShapeKind) -> Result<ConvexShape, PhysicsError> {
    match kind {
        ShapeKind::Cuboid { half } | ShapeKind::Ramp { half, .. } => {
            ConvexShape::cuboid(Vec3::from_array(half))
        }
        ShapeKind::Sphere { radius } => ConvexShape::ball(radius),
        ShapeKind::Capsule {
            half_height,
            radius,
        } => ConvexShape::capsule(half_height, radius),
    }
}

/// Spawns one shape: an entity with `Transform` and `Visual`, and a physics body with its
/// collider. Returns what it spawned.
///
/// # Errors
///
/// Returns a world error when the entity cannot be created or the physics world is missing, or a
/// physics error when a value is rejected (validated shapes make this a broken invariant).
pub fn spawn_shape(world: &mut World, shape: &ShapeDef) -> Result<SpawnedBody, SceneError> {
    spawn_shape_with_metadata(world, shape, ColliderMetadata::default())
}

pub(super) fn spawn_shape_with_metadata(
    world: &mut World,
    shape: &ShapeDef,
    metadata: ColliderMetadata,
) -> Result<SpawnedBody, SceneError> {
    let mut collider = ColliderBuilder::new(collider_shape(shape.kind())?)
        .layers(metadata.layers)
        .surface(metadata.surface)
        .sensor(metadata.sensor);
    if let BodyDef::Dynamic { mass_kg } = shape.body() {
        collider = collider.density(mass_kg / shape.kind().volume())?;
    }
    spawn_shape_with_colliders(world, shape, vec![collider])
}

fn spawn_shape_with_colliders(
    world: &mut World,
    shape: &ShapeDef,
    colliders: Vec<ColliderBuilder>,
) -> Result<SpawnedBody, SceneError> {
    let translation = Vec3::from_array(shape.translation());
    let rotation = rotation_of(shape);
    let full_size = Vec3::from_array(shape.kind().full_size());
    // Everything that can be refused is computed before the world changes.
    let transform = Transform::new(translation, rotation, full_size)?;
    let builder = match shape.body() {
        BodyDef::Static => BodyBuilder::static_body(),
        BodyDef::Dynamic { .. } => BodyBuilder::dynamic(),
        BodyDef::Kinematic => BodyBuilder::kinematic(),
    };
    let builder = builder.translation(translation)?.rotation(rotation)?;
    let visual = Visual {
        mesh: MeshKind::of(shape.kind()),
        look: shape.look(),
    };
    world.resource::<PhysicsWorld>()?;
    let id = world.spawn()?;
    match attach(world, id, transform, visual, builder, colliders) {
        Ok(body) => Ok(SpawnedBody {
            id,
            body,
            kind: shape.kind(),
            look: shape.look(),
        }),
        Err(error) => {
            world.despawn(id)?;
            Err(error)
        }
    }
}

/// Gives a new entity its transform, visual and physics body. On failure it leaves no body
/// behind; the caller removes the entity.
fn attach(
    world: &mut World,
    id: NetId,
    transform: Transform,
    visual: Visual,
    builder: BodyBuilder,
    colliders: Vec<ColliderBuilder>,
) -> Result<BodyId, SceneError> {
    *world.get_mut::<Transform>(id)? = transform;
    world.insert(id, visual)?;
    let physics = world.resource_mut::<PhysicsWorld>()?;
    let body = physics.insert_body(builder);
    for collider in colliders {
        if let Err(error) = physics.insert_collider(Some(body), collider) {
            physics.remove_body(body)?;
            return Err(error.into());
        }
    }
    Ok(body)
}

/// The function that spawns one shape. Production passes [`spawn_shape`]; tests pass one that
/// fails part way through a level.
pub(super) type SpawnFn = fn(&mut World, &ShapeDef) -> Result<SpawnedBody, SceneError>;
