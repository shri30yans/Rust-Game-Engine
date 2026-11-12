//! Builder validation: every rejected input maps to its typed error.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use engine_destruction::{DestructionError, Material, StructureBuilder};

use common::{id, spec, unit_material};

#[test]
fn empty_structure_is_rejected() {
    let builder = StructureBuilder::new(common::config(3));
    assert!(matches!(builder.build(), Err(DestructionError::Empty)));
}

#[test]
fn structure_without_an_anchor_is_rejected() {
    let mut builder = StructureBuilder::new(common::config(3));
    let material = unit_material(&mut builder);
    builder
        .add_element(spec(material, 1.0, 0.0, false, 10))
        .unwrap();
    assert!(matches!(builder.build(), Err(DestructionError::NoAnchor)));
}

#[test]
fn component_with_no_path_to_an_anchor_is_rejected_at_build() {
    let mut builder = StructureBuilder::new(common::config(3));
    let material = unit_material(&mut builder);
    builder
        .add_element(spec(material, 1.0, 0.0, true, 10))
        .unwrap();
    builder
        .add_element(spec(material, 1.0, 1.0, false, 10))
        .unwrap();
    assert!(matches!(
        builder.build(),
        Err(DestructionError::Unanchored(found)) if found == id(1)
    ));
}

#[test]
fn self_edge_is_rejected() {
    let mut builder = StructureBuilder::new(common::config(3));
    let material = unit_material(&mut builder);
    let a = builder
        .add_element(spec(material, 1.0, 0.0, true, 10))
        .unwrap();
    assert!(matches!(
        builder.connect(a, a),
        Err(DestructionError::SelfEdge(_))
    ));
}

#[test]
fn connecting_an_unknown_element_is_rejected() {
    let mut builder = StructureBuilder::new(common::config(3));
    let material = unit_material(&mut builder);
    let a = builder
        .add_element(spec(material, 1.0, 0.0, true, 10))
        .unwrap();
    assert!(matches!(
        builder.connect(a, id(9)),
        Err(DestructionError::UnknownElement(_))
    ));
}

#[test]
fn duplicate_edge_in_either_direction_is_rejected_at_build() {
    let mut builder = StructureBuilder::new(common::config(3));
    let material = unit_material(&mut builder);
    let a = builder
        .add_element(spec(material, 1.0, 0.0, true, 10))
        .unwrap();
    let b = builder
        .add_element(spec(material, 1.0, 1.0, false, 10))
        .unwrap();
    builder.connect(a, b).unwrap();
    builder.connect(b, a).unwrap();
    assert!(matches!(
        builder.build(),
        Err(DestructionError::DuplicateEdge(_, _))
    ));
}

#[test]
fn non_positive_material_is_rejected() {
    assert!(matches!(
        Material::new(0.0, 1.0),
        Err(DestructionError::NotPositive { .. })
    ));
    assert!(matches!(
        Material::new(1.0, -2.0),
        Err(DestructionError::NotPositive { .. })
    ));
}

#[test]
fn non_finite_material_is_rejected() {
    assert!(matches!(
        Material::new(f32::NAN, 1.0),
        Err(DestructionError::NotFinite { .. })
    ));
    assert!(matches!(
        Material::new(1.0, f32::INFINITY),
        Err(DestructionError::NotFinite { .. })
    ));
}

#[test]
fn zero_volume_element_is_rejected() {
    let mut builder = StructureBuilder::new(common::config(3));
    let material = unit_material(&mut builder);
    let mut bad = spec(material, 1.0, 0.0, true, 10);
    bad.volume = 0.0;
    assert!(matches!(
        builder.add_element(bad),
        Err(DestructionError::NotPositive { .. })
    ));
}

#[test]
fn non_finite_position_is_rejected() {
    let mut builder = StructureBuilder::new(common::config(3));
    let material = unit_material(&mut builder);
    let mut bad = spec(material, 1.0, 0.0, true, 10);
    bad.position.x = f32::NAN;
    assert!(matches!(
        builder.add_element(bad),
        Err(DestructionError::NotFinite { .. })
    ));
}

#[test]
fn zero_area_is_rejected_when_the_element_is_added() {
    let mut builder = StructureBuilder::new(common::config(3));
    let material = unit_material(&mut builder);
    assert!(matches!(
        builder.add_element(spec(material, 0.0, 0.0, true, 10)),
        Err(DestructionError::NotPositive { .. })
    ));
}
