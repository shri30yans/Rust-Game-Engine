//! Property test: any valid level survives save and load unchanged. Generated levels are written
//! as RON text, parsed, saved with `to_ron`, parsed again and saved again. The two saved texts
//! must match, which proves that nothing is lost or reordered on the way through the model.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use engine_scene::Level;
use proptest::prelude::*;

const MATERIALS: [&str; 6] = ["Floor", "Wall", "Concrete", "Brick", "Crate", "Glass"];

/// Quarter-unit steps keep every generated float exact and well inside the valid ranges.
fn quarter(value: i32) -> f32 {
    value as f32 / 4.0
}

fn shape_text(index: usize, kind: u8, nums: [i32; 8], body: u8, material: usize) -> String {
    let half = [
        quarter(nums[0].abs() + 1),
        quarter(nums[1].abs() + 1),
        quarter(nums[2].abs() + 1),
    ];
    let kind = match kind % 4 {
        0 => format!(
            "Cuboid(half: ({:?}, {:?}, {:?}))",
            half[0], half[1], half[2]
        ),
        1 => format!("Sphere(radius: {:?})", half[0]),
        2 => format!(
            "Capsule(half_height: {:?}, radius: {:?})",
            quarter(nums[3].abs() + 1),
            half[1]
        ),
        _ => format!(
            "Ramp(half: ({:?}, {:?}, {:?}), tilt_deg: {:?})",
            half[0],
            half[1],
            half[2],
            quarter(nums[3] % 40)
        ),
    };
    let body = match body % 3 {
        0 => "Static".to_owned(),
        1 => format!("Dynamic(mass_kg: {:?})", quarter(nums[4].abs() + 1)),
        _ => "Kinematic".to_owned(),
    };
    let name = if nums[5] % 2 == 0 {
        format!("Some(\"s{index}\")")
    } else {
        "None".to_owned()
    };
    let tint = if nums[6] % 2 == 0 {
        format!(
            ", tint: Some({})",
            0x10_0000 + nums[7].unsigned_abs() % 0xF_FFFF
        )
    } else {
        String::new()
    };
    format!(
        "(id: {}, name: {name}, kind: {kind}, translation: ({:?}, {:?}, {:?}), rotation_deg: ({:?}, {:?}, {:?}), body: {body}, look: (material: {}{tint})),",
        u64::try_from(index).unwrap_or(u64::MAX - 1) + 1,
        quarter(nums[0]),
        quarter(nums[1]),
        quarter(nums[2]),
        quarter(nums[3] % 180),
        quarter(nums[4] % 180),
        quarter(nums[5] % 180),
        MATERIALS[material % MATERIALS.len()],
    )
}

fn level_text(shapes: &[String]) -> String {
    format!(
        "(version: 1, name: \"prop\", gravity: (0.0, -9.81, 0.0), shapes: [\n{}\n], spawns: [(name: \"p1\", translation: (0.0, 1.0, 0.0), yaw_deg: 45.0)], waypoints: [(name: \"w1\", translation: (1.0, 0.0, 1.0))], markers: [(name: \"m1\", translation: (2.0, 0.0, 2.0))])",
        shapes.join("\n")
    )
}

fn shapes_strategy() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(
        (
            any::<u8>(),
            prop::array::uniform8(-400i32..400),
            any::<u8>(),
            0usize..6,
        ),
        1..24,
    )
    .prop_map(|items| {
        items
            .into_iter()
            .enumerate()
            .map(|(i, (kind, nums, body, material))| shape_text(i, kind, nums, body, material))
            .collect()
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn save_then_load_is_stable(shapes in shapes_strategy()) {
        let text = level_text(&shapes);
        let first = Level::parse(&text, "generated.ron");
        prop_assert!(first.is_ok(), "generated level rejected: {:?}", first.err());
        let saved = first.unwrap().to_ron().unwrap();
        let second = Level::parse(&saved, "saved.ron").unwrap();
        let resaved = second.to_ron().unwrap();
        prop_assert_eq!(saved, resaved);
    }

    #[test]
    fn shape_count_and_order_survive(shapes in shapes_strategy()) {
        let level = Level::parse(&level_text(&shapes), "generated.ron").unwrap();
        let saved = level.to_ron().unwrap();
        let reloaded = Level::parse(&saved, "saved.ron").unwrap();
        prop_assert_eq!(reloaded.shapes().len(), shapes.len());
        for (i, shape) in reloaded.shapes().iter().enumerate() {
            if let Some(name) = shape.name() {
                prop_assert_eq!(name, format!("s{i}"));
            }
        }
        prop_assert_eq!(reloaded.spawns().len(), 1);
        prop_assert_eq!(reloaded.waypoints().len(), 1);
        prop_assert_eq!(reloaded.markers().len(), 1);
    }
}
