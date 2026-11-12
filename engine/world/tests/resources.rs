//! Resource store behaviour through the public `World` API: typed access, errors,
//! independence between types, and how resources enter the state hash.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_core::StateHasher;
use engine_world::{Resource, World, WorldError};
use proptest::prelude::*;

#[derive(Debug, PartialEq)]
struct Clock(u64);

impl Resource for Clock {
    fn hash_into(&self, hasher: &mut StateHasher) {
        hasher.write_u64(self.0);
    }
}

#[derive(Debug, PartialEq)]
struct Flag(bool);

impl Resource for Flag {
    fn hash_into(&self, hasher: &mut StateHasher) {
        hasher.write_u8(u8::from(self.0));
    }
}

fn world_with_clock(start: u64) -> World {
    let mut world = World::new(7).unwrap();
    world.insert_resource(Clock(start)).unwrap();
    world
}

#[test]
fn inserted_resource_reads_back() {
    let world = world_with_clock(42);
    assert_eq!(world.resource::<Clock>().unwrap(), &Clock(42));
}

#[test]
fn resource_mut_changes_the_stored_value() {
    let mut world = world_with_clock(1);
    world.resource_mut::<Clock>().unwrap().0 = 9;
    assert_eq!(world.resource::<Clock>().unwrap(), &Clock(9));
}

#[test]
fn duplicate_insert_is_rejected_and_keeps_the_first_value() {
    let mut world = world_with_clock(1);
    assert!(matches!(
        world.insert_resource(Clock(2)),
        Err(WorldError::DuplicateResource(_))
    ));
    assert_eq!(world.resource::<Clock>().unwrap(), &Clock(1));
}

#[test]
fn missing_resource_is_an_error_for_read_and_write() {
    let mut world = World::new(7).unwrap();
    assert!(matches!(
        world.resource::<Clock>(),
        Err(WorldError::MissingResource(_))
    ));
    assert!(matches!(
        world.resource_mut::<Clock>(),
        Err(WorldError::MissingResource(_))
    ));
}

#[test]
fn resource_types_are_independent() {
    let mut world = world_with_clock(3);
    world.insert_resource(Flag(false)).unwrap();
    world.resource_mut::<Flag>().unwrap().0 = true;
    assert_eq!(world.resource::<Clock>().unwrap(), &Clock(3));
    assert_eq!(world.resource::<Flag>().unwrap(), &Flag(true));
}

#[test]
fn state_hash_changes_when_a_hashed_value_changes() {
    let mut world = world_with_clock(1);
    let before = world.state_hash().unwrap();
    world.resource_mut::<Clock>().unwrap().0 = 2;
    assert_ne!(before, world.state_hash().unwrap());
}

#[test]
fn equal_worlds_with_equal_resources_hash_equal() {
    let a = world_with_clock(5);
    let b = world_with_clock(5);
    assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
}

#[test]
fn state_hash_differs_with_and_without_a_resource() {
    let without = World::new(7).unwrap();
    let with = world_with_clock(0);
    assert_ne!(without.state_hash().unwrap(), with.state_hash().unwrap());
}

#[test]
fn state_hash_does_not_depend_on_allocation_addresses() {
    let plain = world_with_clock(8);

    // Live heap blocks shift where the next world's resource is allocated.
    let _noise: Vec<Box<[u8; 64]>> = (0..32).map(|_| Box::new([0_u8; 64])).collect();
    let shifted = world_with_clock(8);

    assert_eq!(plain.state_hash().unwrap(), shifted.state_hash().unwrap());
}

#[test]
fn insertion_order_is_part_of_the_hash() {
    let mut a = World::new(7).unwrap();
    a.insert_resource(Flag(true)).unwrap();
    a.insert_resource(Clock(0)).unwrap();
    let mut b = World::new(7).unwrap();
    b.insert_resource(Clock(0)).unwrap();
    b.insert_resource(Flag(true)).unwrap();
    assert_ne!(a.state_hash().unwrap(), b.state_hash().unwrap());
}

#[derive(Debug, Clone, Copy)]
enum Op {
    Add(u64),
    Set(u64),
    Toggle,
}

fn apply(world: &mut World, op: Op) {
    match op {
        Op::Add(delta) => {
            let clock = world.resource_mut::<Clock>().unwrap();
            clock.0 = clock.0.wrapping_add(delta);
        }
        Op::Set(value) => world.resource_mut::<Clock>().unwrap().0 = value,
        Op::Toggle => {
            let flag = world.resource_mut::<Flag>().unwrap();
            flag.0 = !flag.0;
        }
    }
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        any::<u64>().prop_map(Op::Add),
        any::<u64>().prop_map(Op::Set),
        Just(Op::Toggle),
    ]
}

proptest! {
    #[test]
    fn same_operation_sequence_gives_equal_hashes(
        ops in proptest::collection::vec(op_strategy(), 0..64),
    ) {
        let mut a = world_with_clock(0);
        let mut b = world_with_clock(0);
        a.insert_resource(Flag(false)).unwrap();
        b.insert_resource(Flag(false)).unwrap();
        for op in &ops {
            apply(&mut a, *op);
            apply(&mut b, *op);
        }
        prop_assert_eq!(a.state_hash().unwrap(), b.state_hash().unwrap());
        prop_assert_eq!(a.resource::<Clock>().unwrap(), b.resource::<Clock>().unwrap());
    }
}
