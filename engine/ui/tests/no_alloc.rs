//! Proves that focus navigation allocates nothing once scratch buffers are warm.
//!
//! This file holds a single test on purpose: the counting allocator is process-wide,
//! so a second test running in parallel would pollute the measurement.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_ui::{
    Bindings, Document, FocusNav, FocusRect, MetricsKey, NavInput, TextMeasure, UiLayout, WidgetId,
};
use glam::Vec2;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;
use std::time::Instant;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn grid_rects() -> Vec<FocusRect> {
    let mut out = Vec::with_capacity(9);
    let cell = 100.0;
    for row in 0u8..3 {
        for col in 0u8..3 {
            let id = u32::from(row) * 3 + u32::from(col) + 1;
            let min = Vec2::new(f32::from(col) * cell, f32::from(row) * cell);
            let max = min + Vec2::splat(cell);
            out.push(FocusRect {
                id: WidgetId(id),
                min,
                max,
            });
        }
    }
    out
}

#[test]
fn navigation_allocates_nothing_after_warm_up() {
    let rects = grid_rects();
    let mut nav = FocusNav::with_capacity(9);
    for _ in 0..4 {
        let _ = nav.navigate(WidgetId(5), NavInput::Right, &rects, &[]);
        let _ = nav.navigate(WidgetId(5), NavInput::Next, &rects, &[]);
    }

    let region = Region::new(GLOBAL);
    let _ = nav.navigate(WidgetId(5), NavInput::Down, &rects, &[]);
    let _ = nav.navigate(WidgetId(5), NavInput::Prev, &rects, &[]);
    let stats = region.change();
    assert_eq!(stats.allocations, 0, "navigate allocated: {stats:?}");
    assert_eq!(stats.reallocations, 0, "navigate reallocated: {stats:?}");

    let region = Region::new(GLOBAL);
    let probe: Vec<u8> = Vec::with_capacity(4096);
    let probe_stats = region.change();
    assert!(probe_stats.allocations >= 1, "counter missed allocation");
    drop(probe);

    let source = r##"(version: 1, root: (layout: (display: Flex), children: [
      (layout: (display: Flex), content: Text(text: "SCORE {score}", size: 18.0, color: "#ffffff")),
      (layout: (display: Flex, width: Px(180.0), height: Px(20.0)), content: Bar(value: "health", color: "#ff0000")),
      (show_if: Some("visible"), layout: (display: Flex, height: Px(20.0)), content: Text(text: "READY", size: 14.0, color: "#ffffff"))
    ]))"##;
    let doc = Document::from_ron(source).unwrap();
    let mut bindings = Bindings::new();
    bindings.set_text("score", "0");
    bindings.set_num("health", 0.25);
    bindings.set_flag("visible", true);
    let mut layout = UiLayout::new(doc.clone()).unwrap();
    let mut measure = Mono;
    for _ in 0..2 {
        let _ = layout
            .frame(&bindings, (1280.0, 720.0), &mut measure)
            .unwrap();
    }

    let text_region = Region::new(GLOBAL);
    bindings.set_text("score", "1");
    bindings.set_num("health", 0.5);
    let _ = layout
        .frame(&bindings, (1280.0, 720.0), &mut measure)
        .unwrap();
    let text_stats = text_region.change();
    assert_eq!(
        text_stats.allocations, 0,
        "same-width text/paint update allocated: {text_stats:?}"
    );
    assert_eq!(
        text_stats.reallocations, 0,
        "same-width text/paint update reallocated: {text_stats:?}"
    );

    let steady_region = Region::new(GLOBAL);
    let steady_started = Instant::now();
    for _ in 0..1_000 {
        let _ = layout
            .frame(&bindings, (1280.0, 720.0), &mut measure)
            .unwrap();
    }
    let steady_elapsed = steady_started.elapsed();
    let steady_stats = steady_region.change();
    assert_eq!(
        steady_stats.allocations, 0,
        "steady UI allocated: {steady_stats:?}"
    );
    assert_eq!(
        steady_stats.reallocations, 0,
        "steady UI reallocated: {steady_stats:?}"
    );

    let fresh_started = Instant::now();
    for _ in 0..1_000 {
        let mut fresh = UiLayout::new(doc.clone()).unwrap();
        let _ = fresh
            .frame(&bindings, (1280.0, 720.0), &mut measure)
            .unwrap();
    }
    let fresh_elapsed = fresh_started.elapsed();
    eprintln!(
        "steady retained HUD: 1,000 frames in {steady_elapsed:?}; fresh tree + layout: {fresh_elapsed:?}"
    );

    let region = Region::new(GLOBAL);
    let started = Instant::now();
    for frame in 0..1_000 {
        bindings.set_text("score", if frame % 2 == 0 { "1" } else { "2" });
        bindings.set_num("health", (frame % 100) as f32 / 100.0);
        bindings.set_flag("visible", frame % 2 == 1);
        let _ = layout
            .frame(&bindings, (1280.0, 720.0), &mut measure)
            .unwrap();
    }
    let elapsed = started.elapsed();
    let stats = region.change();
    assert_eq!(
        stats.allocations, 2_000,
        "dirty taffy layout allocation count changed: {stats:?}"
    );
    assert_eq!(stats.reallocations, 0, "retained UI reallocated: {stats:?}");
    assert_eq!(
        stats.bytes_allocated, 744_000,
        "dirty taffy layout allocation size changed: {stats:?}"
    );
    eprintln!("1,000 changing HUD frames: {elapsed:?}, taffy allocations: {stats:?}");
}

struct Mono;
impl TextMeasure for Mono {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(0)
    }

    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * 0.5 * text.chars().count() as f32
    }
}
