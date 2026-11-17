//! The retained, cooked IMAGINE HUD layout and installation of its drawn touch regions allocate
//! nothing on unchanged frames after warm-up. This test deliberately has no GPU fixture.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::alloc::System;

use app::GameEntry;
use engine_ui::{Document, MetricsKey, TextMeasure, UiLayout};
use imagine::ImagineEntry;
use imagine::hud::{BOARD_ROWS, BoardRow, Controls, FeedRow, HudBindings, HudState};
use platform::Input;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const SCREEN: (f32, f32) = (2800.0, 1280.0);
const FRAMES: usize = 1_000;

struct ApproxMeasure;

impl TextMeasure for ApproxMeasure {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(1)
    }

    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * 0.55 * text.chars().count() as f32
    }
}

fn representative_hud() -> HudState {
    HudState {
        health: 82,
        max_health: 100,
        ammo: 17,
        magazine: 30,
        rifle_name: "PRACTICE RIFLE".to_owned(),
        player_name: "HALCYON".to_owned(),
        board: (0..BOARD_ROWS)
            .map(|index| BoardRow {
                name: format!("PLAYER {index}"),
                kills: u32::try_from(index).unwrap_or(0),
                is_player: index == 0,
            })
            .collect(),
        feed: (0..4)
            .map(|index| {
                Some(FeedRow {
                    killer: format!("KILLER {index}"),
                    victim: format!("VICTIM {index}"),
                    killer_is_player: index == 0,
                    victim_is_player: false,
                })
            })
            .collect(),
        ..HudState::default()
    }
}

#[test]
fn unchanged_cooked_hud_layout_and_region_installation_allocate_nothing() {
    let stream = platform::ContentSource::headless()
        .open_content(ImagineEntry::CONTENT.as_str())
        .expect("IMAGINE pack opens");
    let content = engine_assets::PackReader::read(stream).expect("IMAGINE pack validates");
    let path = engine_assets::AssetPath::new("ui/hud.ui.ron").expect("HUD path");
    let document =
        Document::from_ron(content.load_text(&path).expect("HUD in pack")).expect("HUD parses");
    let mut layout = UiLayout::new(document).expect("HUD layout builds");
    let mut bindings = HudBindings::default();
    let values = bindings.update(
        &representative_hud(),
        Controls {
            touch: true,
            left_fire_setting: true,
            ..Controls::default()
        },
    );
    let mut measure = ApproxMeasure;
    let mut input = Input::default();
    input.screen = glam::Vec2::new(SCREEN.0, SCREEN.1);

    for _ in 0..2 {
        let regions = layout
            .frame(values, SCREEN, &mut measure)
            .expect("HUD lays out")
            .touch_regions();
        assert!(input.set_touch_regions(regions));
    }

    let region = Region::new(GLOBAL);
    for _ in 0..FRAMES {
        let regions = layout
            .frame(values, SCREEN, &mut measure)
            .expect("unchanged HUD lays out")
            .touch_regions();
        assert!(input.set_touch_regions(regions));
    }
    let stats = region.change();
    assert_eq!(stats.allocations, 0, "steady HUD allocated: {stats:?}");
    assert_eq!(stats.reallocations, 0, "steady HUD reallocated: {stats:?}");
    assert_eq!(
        stats.bytes_allocated, 0,
        "steady HUD allocated bytes: {stats:?}"
    );

    let probe = Region::new(GLOBAL);
    let allocation_probe: Vec<u8> = Vec::with_capacity(4096);
    assert!(
        probe.change().allocations >= 1,
        "allocation counter did not observe Vec growth"
    );
    drop(allocation_probe);
}
