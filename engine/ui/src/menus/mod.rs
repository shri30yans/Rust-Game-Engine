//! Menu screen stack, input routing, focus and timed transitions.
//!
//! Exiting screens remain drawable but cannot receive input; event and touch buffers retain their
//! capacity after first use. Public paths remain available through `engine_ui::menus`.

mod focus;
mod input;
mod runtime;
mod transition;
mod widgets;

#[cfg(test)]
mod tests;

pub use input::{
    MenuDraw, MenuInput, MenuMotion, MenuTouch, MenuTouchPhase, PointerInput, Transition, UiEvent,
};
pub use runtime::Menus;
pub use transition::cubic_bezier;
