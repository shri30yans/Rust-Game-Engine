//! Platform-neutral menu input and frame event types.

use glam::Vec2;

use crate::focus::{NavInput, RowIndex, WidgetId};
use crate::layout::DrawList;
use crate::stack::{Layer, ScreenId};
use crate::theme::MotionEntry;

pub(super) const INITIAL_EVENT_CAPACITY: usize = 32;
pub(super) const MAX_TOUCHES: usize = 16;
pub(super) const REDUCED_MOTION_MS: u32 = 120;

/// Transition timings and distance supplied by the theme owner.
#[derive(Clone, Debug)]
pub struct MenuMotion {
    /// Screen push timing and curve.
    pub screen: MotionEntry,
    /// Modal push timing and curve.
    pub modal: MotionEntry,
    /// Old-screen fade duration.
    pub exit_ms: u32,
    /// New-screen horizontal offset at the start of a push.
    pub slide_px: f32,
}

/// Pointer state independent of a windowing library.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerInput {
    /// Pointer location in screen pixels.
    pub position: Vec2,
    /// Primary pointer button is held.
    pub down: bool,
    /// Primary pointer button went down this frame.
    pub pressed: bool,
    /// Primary pointer button was released this frame.
    pub released: bool,
}

/// Touch phase independent of a platform library.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuTouchPhase {
    /// Finger touched the surface.
    Began,
    /// Finger moved on the surface.
    Moved,
    /// Finger lifted from the surface.
    Ended,
}

/// A touch change for one menu frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuTouch {
    /// Platform touch identifier.
    pub id: u64,
    /// Position in pixels.
    pub position: Vec2,
    /// Change phase.
    pub phase: MenuTouchPhase,
}

/// Device-neutral controls consumed by [`Menus`].
#[derive(Clone, Debug)]
pub struct MenuInput {
    /// Mouse or pointer state.
    pub pointer: PointerInput,
    touches: Vec<MenuTouch>,
    dropped_touches: usize,
    /// One navigation direction pressed this frame.
    pub nav: Option<NavInput>,
    /// Activate the focused widget.
    pub confirm: bool,
    /// Invoke the screen's Back rule.
    pub back: bool,
    /// Move to the previous tab.
    pub tab_prev: bool,
    /// Move to the next tab.
    pub tab_next: bool,
    /// Scroll delta in lines.
    pub scroll: f32,
}

impl Default for MenuInput {
    fn default() -> Self {
        Self::new()
    }
}
impl MenuInput {
    /// Creates an input frame with storage for the maximum platform touch changes.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pointer: PointerInput::default(),
            touches: Vec::with_capacity(MAX_TOUCHES),
            dropped_touches: 0,
            nav: None,
            confirm: false,
            back: false,
            tab_prev: false,
            tab_next: false,
            scroll: 0.0,
        }
    }

    /// Touch changes recorded in this input frame.
    #[must_use]
    pub fn touches(&self) -> &[MenuTouch] {
        &self.touches
    }

    /// Number of touch changes dropped after the fixed-capacity buffer filled.
    #[must_use]
    pub const fn dropped_touches(&self) -> usize {
        self.dropped_touches
    }

    /// Adds a touch change unless the fixed-capacity buffer is full.
    pub fn push_touch(&mut self, touch: MenuTouch) {
        if self.touches.len() < MAX_TOUCHES {
            self.touches.push(touch);
        } else {
            self.dropped_touches = self.dropped_touches.saturating_add(1);
        }
    }

    /// Clears touch changes and overflow count before filling the next input frame.
    pub fn clear_touches(&mut self) {
        self.touches.clear();
        self.dropped_touches = 0;
    }
}

/// A game-facing menu result associated with a document-resolved widget handle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiEvent {
    /// A button action was activated.
    Action(WidgetId, Option<RowIndex>),
    /// A toggle changed value.
    Toggled(WidgetId, Option<RowIndex>, bool),
    /// A slider changed value.
    Changed(WidgetId, Option<RowIndex>, f32),
    /// A choice or tab selection changed.
    Selected(WidgetId, Option<RowIndex>, u16),
    /// A screen was popped by Back.
    Back(ScreenId),
}

/// Transition values for one drawable screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    /// Opacity multiplier.
    pub opacity: f32,
    /// Horizontal offset in pixels.
    pub offset_x: f32,
    /// Uniform scale.
    pub scale: f32,
}

/// One screen's current draw list and presentation transition.
pub struct MenuDraw<'a> {
    /// Screen identifier.
    pub screen: ScreenId,
    /// Layer this screen belongs to.
    pub layer: Layer,
    /// Retained document draw list.
    pub list: &'a DrawList,
    /// Transition transform at the last menu frame.
    pub transition: Transition,
}
