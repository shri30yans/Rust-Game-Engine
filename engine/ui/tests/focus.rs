//! Integration tests for spatial focus navigation.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_ui::{FocusNav, FocusOverride, FocusRect, NavInput, WidgetId};
use glam::Vec2;
use proptest::prelude::*;

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
fn grid_spatial_navigation() {
    let rects = grid_rects();
    let mut nav = FocusNav::with_capacity(9);
    let centre = WidgetId(5);

    assert_eq!(
        nav.navigate(centre, NavInput::Up, &rects, &[]),
        Some(WidgetId(2))
    );
    assert_eq!(
        nav.navigate(centre, NavInput::Down, &rects, &[]),
        Some(WidgetId(8))
    );
    assert_eq!(
        nav.navigate(centre, NavInput::Left, &rects, &[]),
        Some(WidgetId(4))
    );
    assert_eq!(
        nav.navigate(centre, NavInput::Right, &rects, &[]),
        Some(WidgetId(6))
    );
}

#[test]
fn override_wins() {
    let rects = grid_rects();
    let mut nav = FocusNav::with_capacity(9);
    let overrides = [FocusOverride {
        from: WidgetId(5),
        input: NavInput::Up,
        to: WidgetId(9),
    }];
    assert_eq!(
        nav.navigate(WidgetId(5), NavInput::Up, &rects, &overrides),
        Some(WidgetId(9))
    );
}

#[test]
fn no_candidate_returns_none() {
    let rects = vec![FocusRect {
        id: WidgetId(1),
        min: Vec2::ZERO,
        max: Vec2::splat(10.0),
    }];
    let mut nav = FocusNav::with_capacity(1);
    assert_eq!(
        nav.navigate(WidgetId(1), NavInput::Right, &rects, &[]),
        None
    );
}

#[test]
fn never_returns_current() {
    let rects = grid_rects();
    let mut nav = FocusNav::with_capacity(9);
    for &input in &[
        NavInput::Up,
        NavInput::Down,
        NavInput::Left,
        NavInput::Right,
        NavInput::Next,
        NavInput::Prev,
    ] {
        for rect in &rects {
            if let Some(next) = nav.navigate(rect.id, input, &rects, &[]) {
                assert_ne!(next, rect.id);
            }
        }
    }
}

proptest! {
    #[test]
    fn result_is_always_a_known_widget(
        current in 1u32..=9,
        input in any_nav_input(),
    ) {
        let rects = grid_rects();
        let mut nav = FocusNav::with_capacity(9);
        if let Some(next) = nav.navigate(WidgetId(current), input, &rects, &[]) {
            prop_assert!(rects.iter().any(|r| r.id == next));
            prop_assert_ne!(next, WidgetId(current));
        }
    }
}

fn any_nav_input() -> impl Strategy<Value = NavInput> {
    prop_oneof![
        Just(NavInput::Up),
        Just(NavInput::Down),
        Just(NavInput::Left),
        Just(NavInput::Right),
        Just(NavInput::Next),
        Just(NavInput::Prev),
    ]
}
