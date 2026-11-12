//! Model parsing, validation, and round-trip tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::*;
use crate::SceneError;
use engine_assets::format::AssetId;

/// A valid level with one floor shape. Tests replace parts of it.
const VALID: &str = r#"(
        version: 1,
        name: "test",
        gravity: (0.0, -9.81, 0.0),
        shapes: [
            (
                id: 1,
                name: Some("floor"),
                kind: Cuboid(half: (10.0, 0.5, 10.0)),
                translation: (0.0, -0.5, 0.0),
                body: Static,
                look: (material: Floor),
            ),
        ],
    )"#;

fn invalid_reason(text: &str) -> InvalidReason {
    match Level::parse(text, "test.ron") {
        Err(SceneError::Invalid { error, .. }) => error.reason,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[test]
fn a_valid_level_parses_with_its_shapes_in_order() {
    let level = Level::parse(VALID, "test.ron").expect("valid level");
    assert_eq!(level.version(), SUPPORTED_VERSION);
    assert_eq!(level.name(), "test");
    let floor = level.shapes().first().expect("one shape");
    assert_eq!(level.shapes().len(), 1);
    assert_eq!(floor.name(), Some("floor"));
    assert_eq!(floor.body(), BodyDef::Static);
    assert!(level.instances().is_empty());
    assert!(level.colliders().is_empty());
    assert!(level.prefabs().is_empty());
    assert!(level.volumes().is_empty());
    assert!(level.layouts().is_empty());
    assert!(level.cues().is_empty());
}

#[test]
fn the_extended_level_round_trips_and_keeps_stable_ids() {
    let text = VALID.replace(
            "        ],\n    )",
            "        ],\n        instances: [(asset_id: 42, transform: (translation: (1.0, 2.0, 3.0)))],\n        colliders: [(asset_id: 46, transform: (translation: (1.0, 2.0, 3.0)))],\n        prefabs: [(id: 43, prefab_id: 44, transform: (translation: (0.0, 0.0, 0.0)), overrides: {})],\n        volumes: [(id: 45, name: \"capture\", kind: Zone, center: (0.0, 0.0, 0.0), half_extents: (2.0, 1.0, 2.0))],\n        layouts: [(name: \"open\", mover_poses: {42: (translation: (0.0, 0.0, 0.0))}, switch_states: {43: true})],\n        cues: [(name: \"lockdown\", layout: \"open\", duration_s: 0.5)],\n    )",
        );
    let level = Level::parse(&text, "extended.ron").expect("extended level");
    assert_eq!(level.instances().len(), 1);
    assert_eq!(level.colliders().len(), 1);
    assert_eq!(level.prefabs().len(), 1);
    assert_eq!(level.volumes().len(), 1);
    assert_eq!(level.layouts().len(), 1);
    assert_eq!(level.cues().len(), 1);
    let encoded = level.to_ron().expect("encode");
    assert_eq!(
        Level::parse(&encoded, "roundtrip.ron").expect("decode"),
        level
    );
}

#[test]
fn an_asset_id_of_zero_is_rejected() {
    let text = VALID.replace(
            "        ],\n    )",
            "        ],\n        instances: [(asset_id: 0, transform: (translation: (0.0, 0.0, 0.0)))],\n    )",
        );
    assert!(matches!(
        Level::parse(&text, "zero-id.ron"),
        Err(SceneError::Parse { .. })
    ));
}

#[test]
fn an_instance_with_non_positive_scale_is_rejected() {
    let text = VALID.replace(
            "        ],\n    )",
            "        ],\n        instances: [(asset_id: 42, transform: (translation: (0.0, 0.0, 0.0), scale: (1.0, 0.0, 1.0)))],\n    )",
        );
    assert_eq!(
        invalid_reason(&text),
        InvalidReason::NotPositive { field: "scale" }
    );
}

#[test]
fn a_cue_cannot_select_an_unknown_layout() {
    let text = VALID.replace(
            "        ],\n    )",
            "        ],\n        layouts: [(name: \"open\")],\n        cues: [(name: \"lockdown\", layout: \"missing\", duration_s: 0.5)],\n    )",
        );
    assert_eq!(
        invalid_reason(&text),
        InvalidReason::UnknownLayout("missing".to_owned())
    );
}

#[test]
fn an_unsupported_version_is_refused() {
    let text = VALID.replace("version: 1", "version: 2");
    assert_eq!(
        invalid_reason(&text),
        InvalidReason::Version {
            found: 2,
            expected: SUPPORTED_VERSION
        }
    );
}

#[test]
fn a_zero_extent_is_refused_with_the_shape_location() {
    let text = VALID.replace("half: (10.0, 0.5, 10.0)", "half: (10.0, 0.0, 10.0)");
    let err = match Level::parse(&text, "test.ron") {
        Err(err) => err,
        Ok(_) => panic!("zero extent accepted"),
    };
    let message = err.to_string();
    assert!(message.contains("shape 0"), "{message}");
    assert!(message.contains("greater than zero"), "{message}");
}

#[test]
fn a_negative_radius_is_refused() {
    let text = VALID.replace(
        "kind: Cuboid(half: (10.0, 0.5, 10.0))",
        "kind: Sphere(radius: -1.0)",
    );
    assert_eq!(
        invalid_reason(&text),
        InvalidReason::NotPositive { field: "radius" }
    );
}

#[test]
fn a_non_positive_mass_is_refused() {
    let text = VALID.replace("body: Static", "body: Dynamic(mass_kg: 0.0)");
    assert_eq!(
        invalid_reason(&text),
        InvalidReason::NotPositive { field: "mass_kg" }
    );
}

#[test]
fn duplicate_shape_names_are_refused() {
    let shape = r#"(
                id: 2,
                name: Some("floor"),
                kind: Sphere(radius: 1.0),
                translation: (0.0, 0.0, 0.0),
                body: Static,
                look: (material: Wall),
            ),"#;
    let text = VALID.replace("        ],\n    )", &format!("{shape}\n        ],\n    )"));
    assert_eq!(
        invalid_reason(&text),
        InvalidReason::DuplicateName("floor".to_owned())
    );
}

#[test]
fn an_empty_shape_name_is_refused() {
    let text = VALID.replace("Some(\"floor\")", "Some(\"\")");
    assert_eq!(invalid_reason(&text), InvalidReason::EmptyName);
}

#[test]
fn an_unknown_material_is_a_parse_error() {
    let text = VALID.replace("material: Floor", "material: Lava");
    assert!(matches!(
        Level::parse(&text, "test.ron"),
        Err(SceneError::Parse { .. })
    ));
}

#[test]
fn an_unknown_field_is_a_parse_error() {
    let text = VALID.replace("gravity:", "gravty:");
    assert!(matches!(
        Level::parse(&text, "test.ron"),
        Err(SceneError::Parse { .. })
    ));
    let text = VALID.replace(
        "look: (material: Floor)",
        "look: (material: Floor, shiny: 1)",
    );
    assert!(matches!(
        Level::parse(&text, "test.ron"),
        Err(SceneError::Parse { .. })
    ));
}

#[test]
fn a_colour_wider_than_24_bits_is_refused() {
    let text = VALID.replace(
        "look: (material: Floor)",
        "look: (material: Floor, tint: Some(16777216))",
    );
    assert_eq!(invalid_reason(&text), InvalidReason::WideColour(16_777_216));
}

#[test]
fn a_custom_roughness_outside_the_unit_range_is_refused() {
    let text = VALID.replace(
        "look: (material: Floor)",
        "look: (material: Custom(colour_hex: 0, roughness: 1.5, metallic: 0.0))",
    );
    assert_eq!(
        invalid_reason(&text),
        InvalidReason::OutsideUnit { field: "roughness" }
    );
}

#[test]
fn a_non_finite_number_is_refused_by_the_shape_constructor() {
    let look = Look {
        material: MaterialKind::Floor,
        tint: None,
    };
    let reason = ShapeDef::new(
        AssetId::new(1).expect("asset id"),
        None,
        ShapeKind::Sphere { radius: f32::NAN },
        [0.0; 3],
        [0.0; 3],
        BodyDef::Static,
        look,
    )
    .expect_err("NaN radius accepted");
    assert_eq!(reason, InvalidReason::NonFinite { field: "radius" });
    let reason = ShapeDef::new(
        AssetId::new(1).expect("asset id"),
        None,
        ShapeKind::Sphere { radius: 1.0 },
        [f32::INFINITY, 0.0, 0.0],
        [0.0; 3],
        BodyDef::Static,
        look,
    )
    .expect_err("infinite translation accepted");
    assert_eq!(
        reason,
        InvalidReason::NonFinite {
            field: "translation"
        }
    );
}

#[test]
fn a_missing_shape_id_is_refused_at_the_load_boundary() {
    let text = VALID.replace("                id: 1,\n", "");
    assert!(matches!(
        Level::parse(&text, "missing-id.ron"),
        Err(SceneError::Parse { .. })
    ));
}

#[test]
fn source_ids_are_unique_across_shape_and_cooked_records() {
    let text = VALID.replace(
            "        ],\n    )",
            "        ],\n        instances: [(asset_id: 1, transform: (translation: (0.0, 0.0, 0.0)))],\n    )",
        );
    let error = Level::parse(&text, "duplicate-id.ron").expect_err("duplicate accepted");
    let message = error.to_string();
    assert!(message.contains("asset id 1"), "{message}");
}

#[test]
fn a_non_finite_gravity_is_refused() {
    let text = VALID.replace("gravity: (0.0, -9.81, 0.0)", "gravity: (0.0, 1e39, 0.0)");
    let err = Level::parse(&text, "test.ron");
    assert!(
        matches!(
            err,
            Err(SceneError::Invalid { .. } | SceneError::Parse { .. })
        ),
        "{err:?}"
    );
}

#[test]
fn a_level_over_the_shape_limit_is_refused() {
    let shape = "(id: 1, kind: Sphere(radius: 1.0), translation: (0.0, 0.0, 0.0), body: Static, look: (material: Wall)),";
    let shapes = shape.repeat(MAX_SHAPES + 1);
    let text =
        format!("(version: 1, name: \"big\", gravity: (0.0, -9.81, 0.0), shapes: [{shapes}])");
    assert_eq!(
        invalid_reason(&text),
        InvalidReason::TooManyShapes {
            count: MAX_SHAPES + 1,
            limit: MAX_SHAPES
        }
    );
}

#[test]
fn duplicate_marker_names_in_one_list_are_refused() {
    let text = VALID.replace(
            "        ],\n    )",
            "        ],\n        spawns: [\n            (name: \"p1\", translation: (0.0, 1.0, 0.0)),\n            (name: \"p1\", translation: (1.0, 1.0, 0.0)),\n        ],\n    )",
        );
    assert_eq!(
        invalid_reason(&text),
        InvalidReason::DuplicateName("p1".to_owned())
    );
}

#[test]
fn the_same_marker_name_in_two_lists_is_allowed() {
    let text = VALID.replace(
            "        ],\n    )",
            "        ],\n        spawns: [(name: \"p1\", translation: (0.0, 1.0, 0.0))],\n        waypoints: [(name: \"p1\", translation: (1.0, 1.0, 0.0))],\n    )",
        );
    Level::parse(&text, "test.ron").expect("same name in different lists");
}

#[test]
fn a_missing_gravity_is_a_parse_error() {
    let text = VALID.replace("gravity: (0.0, -9.81, 0.0),", "");
    assert!(matches!(
        Level::parse(&text, "test.ron"),
        Err(SceneError::Parse { .. })
    ));
}

#[test]
fn the_parse_error_names_the_file() {
    let err = Level::parse("(version: ", "broken.ron").expect_err("broken text");
    assert!(err.to_string().contains("broken.ron"), "{err}");
}

#[test]
fn full_size_and_volume_match_the_shape() {
    let cube = ShapeKind::Cuboid { half: [0.25; 3] };
    assert_eq!(cube.full_size(), [0.5; 3]);
    assert!((cube.volume() - 0.125).abs() < 1e-7);
    let sphere = ShapeKind::Sphere { radius: 0.5 };
    assert_eq!(sphere.full_size(), [1.0; 3]);
    let capsule = ShapeKind::Capsule {
        half_height: 0.5,
        radius: 0.3,
    };
    assert_eq!(capsule.full_size(), [0.6, 1.6, 0.6]);
}
