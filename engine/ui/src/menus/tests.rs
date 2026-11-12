//! Menu runtime behavior tests.

use super::input::MAX_TOUCHES;
use super::*;
use glam::Vec2;

#[test]
fn touch_input_stays_within_its_fixed_capacity() {
    let mut input = MenuInput::new();
    for id in 0..17 {
        input.push_touch(MenuTouch {
            id,
            position: Vec2::ZERO,
            phase: MenuTouchPhase::Began,
        });
    }
    assert_eq!(input.touches().len(), MAX_TOUCHES);
    assert_eq!(input.dropped_touches(), 1);
    input.clear_touches();
    assert!(input.touches().is_empty());
    assert_eq!(input.dropped_touches(), 0);
}
#[test]
fn cubic_bezier_hits_endpoints_and_linear_midpoint() {
    assert_eq!(cubic_bezier(0.0, (0.2, 0.0, 0.2, 1.0)), 0.0);
    assert_eq!(cubic_bezier(1.0, (0.2, 0.0, 0.2, 1.0)), 1.0);
    assert!((cubic_bezier(0.5, (0.0, 0.0, 1.0, 1.0)) - 0.5).abs() < 1.0e-4);
    assert!((cubic_bezier(0.5, (0.25, 0.1, 0.25, 1.0)) - 0.8024).abs() < 0.002);
}
