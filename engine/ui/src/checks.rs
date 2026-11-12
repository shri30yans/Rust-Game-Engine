//! Layout checks run by tests and authoring tools over a finished [`DrawList`].
//!
//! They never run in a shipped frame: a check failure means the authored document is wrong, and
//! the fix is in the document, not at run time.

use engine_core::Rect;

use crate::error::UiError;
use crate::layout::DrawList;

/// Checks all text runs in a draw list against their measured boxes and parent bounds.
///
/// # Errors
///
/// Returns the first text run whose measured glyph width or box extends beyond its parent.
pub fn check_text_overflow(draw_list: &DrawList) -> Result<(), UiError> {
    const LAYOUT_EPSILON_PX: f32 = 0.1;
    for run in draw_list.text_runs() {
        let right = run.rect.x + run.rect.w;
        let bottom = run.rect.y + run.rect.h;
        let parent_right = run.parent.x + run.parent.w;
        let parent_bottom = run.parent.y + run.parent.h;
        if run.width > run.rect.w + LAYOUT_EPSILON_PX
            || run.rect.x < run.parent.x - LAYOUT_EPSILON_PX
            || run.rect.y < run.parent.y - LAYOUT_EPSILON_PX
            || right > parent_right + LAYOUT_EPSILON_PX
            || bottom > parent_bottom + LAYOUT_EPSILON_PX
        {
            return Err(UiError::TextOverflow {
                node: run.node.to_string(),
            });
        }
    }
    Ok(())
}

/// Checks that every visible touch control and menu widget has its complete original rectangle
/// inside `safe_viewport`. Touch hit regions are clipped for safe runtime input, but clipping must
/// never hide an authored layout error in a HUD or menu test.
///
/// # Errors
///
/// Returns the first visible interactive rectangle that escapes `safe_viewport`.
pub fn check_interactive_bounds(draw_list: &DrawList, safe_viewport: Rect) -> Result<(), UiError> {
    if !rect_is_valid(safe_viewport) {
        return Err(UiError::Layout(
            "safe viewport must be finite and positive".to_owned(),
        ));
    }
    for bounds in draw_list.touch_bounds() {
        if !rect_is_inside(bounds.rect, safe_viewport) {
            return Err(UiError::InteractiveOutsideSafeArea {
                kind: "touch control",
                id: usize::from(bounds.id.0),
            });
        }
    }
    for widget in draw_list.widgets() {
        if !rect_is_inside(widget.rect, safe_viewport) {
            return Err(UiError::InteractiveOutsideSafeArea {
                kind: "menu widget",
                id: widget.widget_index,
            });
        }
    }
    Ok(())
}

fn rect_is_valid(rect: Rect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.w.is_finite()
        && rect.h.is_finite()
        && rect.w > 0.0
        && rect.h > 0.0
}

fn rect_is_inside(rect: Rect, container: Rect) -> bool {
    const LAYOUT_EPSILON_PX: f32 = 0.1;
    rect_is_valid(rect)
        && rect.x >= container.x - LAYOUT_EPSILON_PX
        && rect.y >= container.y - LAYOUT_EPSILON_PX
        && rect.x + rect.w <= container.x + container.w + LAYOUT_EPSILON_PX
        && rect.y + rect.h <= container.y + container.h + LAYOUT_EPSILON_PX
}
