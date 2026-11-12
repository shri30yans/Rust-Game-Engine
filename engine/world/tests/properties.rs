//! Property tests for the world's invariants: hierarchy, id reuse, state hashing,
//! propagation and registry round-trips. Everything runs through the public API.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;

use engine_core::{SimRng, Tick};
use engine_world::{
    GlobalTransform, NetId, Replication, SystemContext, Transform, World, WorldError,
    propagate_transforms,
};
use glam::{Mat4, Quat, Vec3};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Health {
    hp: u32,
    label: String,
}

/// One generated operation: a kind selector and two indices into the live set.
type Op = (u8, usize, usize);

fn ops_strategy(max: usize) -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec((any::<u8>(), any::<usize>(), any::<usize>()), 1..max)
}

fn pick(world: &World, index: usize) -> Option<NetId> {
    let ids: Vec<NetId> = world.net_ids().collect();
    if ids.is_empty() {
        None
    } else {
        ids.get(index % ids.len()).copied()
    }
}

/// Applies one op. Only `Cycle` is an expected failure; anything else is a bug.
fn apply(world: &mut World, (kind, a, b): Op) -> Result<(), WorldError> {
    match kind % 4 {
        0 => {
            world.spawn()?;
        }
        1 => {
            if let (Some(child), Some(parent)) = (pick(world, a), pick(world, b)) {
                match world.set_parent(child, parent) {
                    Ok(()) | Err(WorldError::Cycle { .. }) => {}
                    Err(other) => return Err(other),
                }
            }
        }
        2 => {
            if let Some(id) = pick(world, a) {
                world.detach(id)?;
            }
        }
        _ => {
            if let Some(id) = pick(world, a) {
                world.despawn(id)?;
            }
        }
    }
    Ok(())
}

/// True when `node` is `ancestor` or lies below it.
fn is_within(world: &World, node: NetId, ancestor: NetId) -> bool {
    let mut cursor = Some(node);
    let mut steps = 0usize;
    while let Some(current) = cursor {
        if current == ancestor {
            return true;
        }
        steps += 1;
        assert!(steps <= world.len() + 1, "parent walk did not terminate");
        cursor = world.parent(current).unwrap();
    }
    false
}

/// Parent and children views agree, and no walk up returns to its start.
fn assert_forest(world: &World) {
    for id in world.net_ids() {
        if let Some(parent) = world.parent(id).unwrap() {
            assert_ne!(parent, id);
            assert!(world.children(parent).unwrap().contains(&id));
        }
        for child in world.children(id).unwrap() {
            assert_eq!(world.parent(child).unwrap(), Some(id));
        }
        // An entity must not be its own ancestor.
        if let Some(parent) = world.parent(id).unwrap() {
            assert!(!is_within(world, parent, id), "{id:?} is its own ancestor");
        }
    }
}

proptest! {
    #[test]
    fn hierarchy_never_cycles(ops in ops_strategy(200)) {
        let mut world = World::new(9).unwrap();
        for op in ops {
            let (kind, a, b) = op;
            if kind % 4 == 1 {
                if let (Some(child), Some(parent)) = (pick(&world, a), pick(&world, b)) {
                    // set_parent must fail exactly when the parent already lies within the child's subtree.
                    let would_cycle = is_within(&world, parent, child);
                    let result = world.set_parent(child, parent);
                    prop_assert_eq!(result.is_err(), would_cycle);
                    if let Err(err) = result {
                        let is_cycle = matches!(err, WorldError::Cycle { .. });
                        prop_assert!(is_cycle, "rejected with {err:?}");
                    }
                }
            } else {
                apply(&mut world, op).unwrap();
            }
            assert_forest(&world);
        }
    }

    #[test]
    fn net_ids_are_never_reused(ops in ops_strategy(200)) {
        let mut world = World::new(1).unwrap();
        let mut ever_issued = HashSet::new();
        let mut dead = Vec::new();
        for (kind, a, _) in ops {
            let before: HashSet<NetId> = world.net_ids().collect();
            if kind % 2 == 0 {
                let id = world.spawn().unwrap();
                prop_assert!(ever_issued.insert(id), "net id {id:?} was handed out twice");
            } else if let Some(id) = pick(&world, a) {
                world.despawn(id).unwrap();
            }
            let after: HashSet<NetId> = world.net_ids().collect();
            dead.extend(before.difference(&after).copied());
        }
        for id in &dead {
            prop_assert!(!world.is_alive(*id));
            prop_assert!(world.get::<Transform>(*id).is_err());
        }
    }

    #[test]
    fn identical_operations_give_identical_hashes(ops in ops_strategy(150)) {
        let mut a = World::new(5).unwrap();
        let mut b = World::new(5).unwrap();
        for op in ops {
            apply(&mut a, op).unwrap();
            apply(&mut b, op).unwrap();
            prop_assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
        }
    }

    #[test]
    fn any_single_change_changes_the_hash(ops in ops_strategy(100)) {
        let mut world = World::new(5).unwrap();
        world.register::<Health>(Replication::Replicated).unwrap();
        for op in ops {
            apply(&mut world, op).unwrap();
        }
        let target = match pick(&world, 0) {
            Some(id) => id,
            None => world.spawn().unwrap(),
        };
        let before = world.state_hash().unwrap();

        // Moving a transform changes the hash, and moving it back restores it.
        let original = world.get::<Transform>(target).unwrap().translation();
        world
            .get_mut::<Transform>(target)
            .unwrap()
            .set_translation(original + Vec3::X)
            .unwrap();
        prop_assert_ne!(world.state_hash().unwrap(), before);
        world
            .get_mut::<Transform>(target)
            .unwrap()
            .set_translation(original)
            .unwrap();
        prop_assert_eq!(world.state_hash().unwrap(), before);

        // Adding and removing a registered component behaves the same way.
        world
            .insert(target, Health { hp: 3, label: String::from("x") })
            .unwrap();
        prop_assert_ne!(world.state_hash().unwrap(), before);
        world.remove::<Health>(target).unwrap();
        prop_assert_eq!(world.state_hash().unwrap(), before);
    }

    #[test]
    fn global_transform_is_product_of_ancestor_locals(
        chain in prop::collection::vec(
            (-10.0f32..10.0, -10.0f32..10.0, -10.0f32..10.0, -3.0f32..3.0),
            1..40,
        ),
    ) {
        let mut world = World::new(2).unwrap();
        let mut expected = Mat4::IDENTITY;
        let mut previous: Option<NetId> = None;
        let mut last = None;
        for (x, y, z, angle) in chain {
            let id = world.spawn().unwrap();
            let local = Transform::new(
                Vec3::new(x, y, z),
                Quat::from_rotation_z(angle),
                Vec3::new(1.0, 1.0, 1.0),
            )
            .unwrap();
            expected *= local.matrix();
            *world.get_mut::<Transform>(id).unwrap() = local;
            if let Some(parent) = previous {
                world.set_parent(id, parent).unwrap();
            }
            previous = Some(id);
            last = Some(id);
        }
        let mut rng = SimRng::new(3, 0);
        let mut ctx = SystemContext { tick: Tick::ZERO, dt: 1.0 / 60.0, rng: &mut rng };
        propagate_transforms(&mut world, &mut ctx).unwrap();
        let leaf = last.unwrap();
        let got = world.get::<GlobalTransform>(leaf).unwrap().translation();
        let want = expected.w_axis.truncate();
        prop_assert!((got - want).length() <= 1e-3 * (1.0 + want.length()));
    }

    #[test]
    fn registry_round_trips_components(hp in any::<u32>(), label in ".{0,32}") {
        let value = Health { hp, label };
        let mut source = World::new(1).unwrap();
        let mut sink = World::new(1).unwrap();
        let id_source = source.register::<Health>(Replication::Replicated).unwrap();
        let id_sink = sink.register::<Health>(Replication::Replicated).unwrap();
        prop_assert_eq!(id_source, id_sink);
        let duplicate = source.register::<Health>(Replication::OwnerOnly);
        let is_duplicate = matches!(duplicate, Err(WorldError::DuplicateComponent(_)));
        prop_assert!(is_duplicate);

        let entity = source.spawn().unwrap();
        source.insert(entity, value.clone()).unwrap();
        let bytes = source.encode_component(entity, id_source).unwrap().unwrap();

        let target = sink.spawn().unwrap();
        sink.decode_component(target, id_sink, &bytes).unwrap();
        let decoded = (*sink.get::<Health>(target).unwrap()).clone();
        prop_assert_eq!(decoded, value);

        let mut extended = bytes.clone();
        extended.push(0);
        let trailing = sink.decode_component(target, id_sink, &extended);
        let is_trailing = matches!(trailing, Err(WorldError::TrailingBytes(_)));
        prop_assert!(is_trailing);
        prop_assert_eq!(source.registry().class(id_source), Some(Replication::Replicated));
    }
}

#[test]
fn built_in_components_have_fixed_ids() {
    let world = World::new(0).unwrap();
    let transform = world.registry().id_of::<Transform>().unwrap();
    let global = world.registry().id_of::<GlobalTransform>().unwrap();
    assert_eq!(transform.raw(), 0);
    assert_eq!(global.raw(), 1);
    assert_eq!(world.registry().class(global), Some(Replication::Cosmetic));
}

#[test]
fn unregistered_type_cannot_be_inserted_and_encode_of_absent_component_is_none() {
    let mut world = World::new(0).unwrap();
    let id = world.spawn().unwrap();
    let missing = matches!(
        world.get::<Health>(id),
        Err(WorldError::MissingComponent(_, _))
    );
    assert!(missing);
    let unknown = world.insert(
        id,
        Health {
            hp: 1,
            label: String::new(),
        },
    );
    assert!(matches!(unknown, Err(WorldError::UnknownComponent(_))));
    let transform = world.registry().id_of::<Transform>().unwrap();
    assert!(world.encode_component(id, transform).unwrap().is_some());
}
