//! Shared fixtures for the engine-destruction tests and benches.
//!
//! The bench includes this file through `#[path]`, so it stays self-contained
//! and uses only the public API. Test code may use `unwrap` and casts here.
#![allow(
    dead_code,
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::indexing_slicing
)]

use std::num::{NonZeroU16, NonZeroU32};

use engine_core::Tick;
use engine_destruction::{
    DestructionBudget, DestructionConfig, ElementId, ElementSpec, EventSink, Material, MaterialId,
    Structure, StructureBuilder, StructureState,
};
use glam::Vec3;

/// Rules with the given warning length.
pub fn config(warning: u32) -> DestructionConfig {
    DestructionConfig::new(NonZeroU32::new(warning).unwrap())
}

/// Element id for a table index.
pub fn id(raw: usize) -> ElementId {
    ElementId::new(raw as u32)
}

/// Material with density 1 and strength 1, so mass equals volume and capacity equals area.
pub fn unit_material(builder: &mut StructureBuilder) -> MaterialId {
    builder
        .add_material(Material::new(1.0, 1.0).unwrap())
        .unwrap()
}

/// Element with unit volume, capacity `capacity` (through area), position on the x axis, and `max_health`.
pub fn spec(
    material: MaterialId,
    capacity: f32,
    x: f32,
    anchor: bool,
    max_health: u16,
) -> ElementSpec {
    ElementSpec {
        material,
        volume: 1.0,
        area: capacity,
        position: Vec3::new(x, 0.0, 0.0),
        max_health: NonZeroU16::new(max_health).unwrap(),
        anchor,
    }
}

/// Builds a structure from per-element capacities, anchor indices and undirected edges.
/// Element `i` sits at `x = i` and has health 100.
pub fn build(
    capacities: &[f32],
    anchors: &[usize],
    edges: &[(usize, usize)],
    warning: u32,
) -> Structure {
    let mut builder = StructureBuilder::new(config(warning));
    let material = unit_material(&mut builder);
    for (index, capacity) in capacities.iter().enumerate() {
        builder
            .add_element(spec(
                material,
                *capacity,
                index as f32,
                anchors.contains(&index),
                100,
            ))
            .unwrap();
    }
    for &(a, b) in edges {
        builder.connect(id(a), id(b)).unwrap();
    }
    builder.build().unwrap()
}

/// `towers` vertical columns of `height` elements each. Every column's bottom
/// element is an anchor, and the bottoms are joined by a ground row. Each element
/// has capacity `height`, which exactly carries the column above it, so the
/// intact structure settles with nothing overloaded. Element index is `tower * height + floor`.
pub fn ground_towers(towers: usize, height: usize, warning: u32) -> Structure {
    let mut capacities = Vec::with_capacity(towers * height);
    let mut anchors = Vec::with_capacity(towers);
    let mut edges = Vec::new();
    for tower in 0..towers {
        for floor in 0..height {
            let index = tower * height + floor;
            capacities.push(height as f32);
            if floor == 0 {
                anchors.push(index);
                if tower + 1 < towers {
                    edges.push((index, index + height));
                }
            }
            if floor + 1 < height {
                edges.push((index, index + 1));
            }
        }
    }
    build(&capacities, &anchors, &edges, warning)
}

/// Runs steps with a per-step visit budget until the state settles. Returns the step count.
/// Panics if the state does not settle within a generous step limit.
pub fn settle(state: &mut StructureState, sink: &mut EventSink, visits_per_step: u32) -> u32 {
    let budget = DestructionBudget::new(NonZeroU32::new(visits_per_step).unwrap());
    for steps in 1..=1_000_000u32 {
        let report = state.step(Tick::ZERO, sink, budget).unwrap();
        if report.settled {
            return steps;
        }
    }
    panic!("state did not settle");
}
