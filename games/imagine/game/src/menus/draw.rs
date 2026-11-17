//! Drawing and bounds checks for active IMAGINE menu layers.

use app::{AppError, draw_menus};
use engine_core::Rect;
use engine_ui::MenuInput;
use glam::Vec2;
use render::{FontId, Ui};

use super::runtime::GameMenus;

impl GameMenus {
    /// Lays out screens pushed during the current simulation tick before this frame is drawn.
    ///
    /// A fresh loading document has no draw list until it passes through `Menus::frame`; travel
    /// is applied at the next frame boundary, so this pass gives the loading screen one frame.
    pub fn layout_for_draw(&mut self, screen: Vec2) -> Result<(), AppError> {
        self.now_ms = self.now_ms.saturating_add(16);
        self.runtime.frame(
            &MenuInput::new(),
            &self.bindings,
            (screen.x, screen.y),
            self.now_ms,
        )?;
        Ok(())
    }

    /// Checks every currently visible menu draw list for text outside its box or parent.
    ///
    /// # Errors
    ///
    /// Returns a UI error for invalid text bounds.
    pub fn check_text_overflow(&self) -> Result<(), AppError> {
        for draw in self.runtime.draw_list() {
            engine_ui::check_text_overflow(draw.list)?;
        }
        Ok(())
    }

    /// Visible bounds for each active instance of a menu widget.
    #[must_use]
    pub fn widget_rects(&self, name: &str) -> Vec<Rect> {
        self.active
            .iter()
            .rev()
            .filter(|entry| entry.layer.accepts_focus())
            .flat_map(|entry| self.runtime.widget_rects(entry.id, name))
            .collect()
    }

    /// Draws retained menu layers after the game HUD.
    pub fn draw(&self, ui: &mut Ui, font: FontId) {
        draw_menus(&self.runtime, ui, font);
    }
}
