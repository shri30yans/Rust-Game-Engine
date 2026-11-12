//! Maps platform input snapshots to device-neutral menu controls and draws menu layers.
//!
//! Invariants: the mapping reuses the caller's pre-sized touch buffer; menu screens use the same
//! retained draw-list playback as [`crate::UiScreen`].

use engine_ui::{MenuInput, MenuTouch, MenuTouchPhase, Menus, NavInput, PointerInput};
use glam::Vec2;
use platform::{Code, Input, Mouse};
use render::{FontId, Ui};

use crate::UiScreen;

/// Tracks pointer button state and maps one platform frame into `MenuInput`.
#[derive(Debug, Default)]
pub struct MenuInputMapper {
    primary_down: bool,
}

impl MenuInputMapper {
    /// Fills `out` from a platform frame without allocating after touch capacity is established.
    pub fn map(&mut self, input: &Input, out: &mut MenuInput) {
        let down = input.button(Mouse::Left);
        out.pointer = PointerInput {
            position: input.mouse_pos,
            down,
            pressed: input.clicked(Mouse::Left),
            released: self.primary_down && !down,
        };
        self.primary_down = down;
        out.clear_touches();
        for touch in input.touches() {
            let phase = match touch.phase {
                platform::TouchPhase::Began => MenuTouchPhase::Began,
                platform::TouchPhase::Moved => MenuTouchPhase::Moved,
                platform::TouchPhase::Ended => MenuTouchPhase::Ended,
            };
            out.push_touch(MenuTouch {
                id: touch.id,
                position: touch.pos,
                phase,
            });
        }
        out.nav = if input.hit(Code::Up) || input.hit(Code::W) {
            Some(NavInput::Up)
        } else if input.hit(Code::Down) || input.hit(Code::S) {
            Some(NavInput::Down)
        } else if input.hit(Code::Left) || input.hit(Code::A) {
            Some(NavInput::Left)
        } else if input.hit(Code::Right) || input.hit(Code::D) {
            Some(NavInput::Right)
        } else {
            None
        };
        out.confirm = input.hit(Code::Enter) || input.hit(Code::Space);
        out.back = input.hit(Code::Esc) || input.hit(Code::Backspace);
        out.tab_prev = input.hit(Code::Q) || input.hit(Code::PageUp);
        out.tab_next = input.hit(Code::E) || input.hit(Code::PageDown);
        out.scroll = input.wheel;
    }
}

/// Draws every active or exiting menu layer in painter order.
pub fn draw_menus(menus: &Menus, ui: &mut Ui, font: FontId) {
    for screen in menus.draw_list() {
        UiScreen::draw_list(ui, font, screen.list, screen.transition);
    }
}

/// Converts the physical screen size to the engine's menu coordinate space.
#[must_use]
pub fn screen_size(input: &Input) -> Vec2 {
    input.screen
}
