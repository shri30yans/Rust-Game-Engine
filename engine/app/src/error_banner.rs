//! The error banner: a red bar across the top of the screen that shows the latest recoverable
//! frame error or dev-time reload error. A good reload or five seconds without a new error clears it.
//!
//! Invariants: the bar is drawn only while a message is set; the bar shows the first line of the
//! message, so a multi-line error (a parse span, for example) still fits, and the full text is
//! the caller's to log; drawing never fails a frame.

use std::time::{Duration, Instant};

use render::{Align, FontId, Ui, rgba};

/// Height of the bar in pixels.
const BAR_HEIGHT: f32 = 44.0;
/// Text size in pixels.
const TEXT_PX: f32 = 22.0;
/// Left padding of the text in pixels.
const PADDING: f32 = 16.0;
/// Vertical offset of the text's top edge in pixels.
const TEXT_TOP: f32 = 11.0;
/// Bar colour, sRGB hex.
const BAR_HEX: u32 = 0xb3_261e;
/// Time without another error before the banner disappears.
const ERROR_VISIBLE_FOR: Duration = Duration::from_secs(5);

/// Shows one error message at the top of the screen. It holds text only; the font is given when
/// it is drawn, so the message logic needs no GPU.
#[derive(Debug, Clone, Default)]
pub struct ErrorBanner {
    message: Option<String>,
    shown_at: Option<Instant>,
}

impl ErrorBanner {
    /// Shows `message`, replacing any message already shown.
    pub fn show(&mut self, message: impl Into<String>, now: Instant) {
        self.message = Some(message.into());
        self.shown_at = Some(now);
    }

    /// Hides the banner.
    pub fn clear(&mut self) {
        self.message = None;
        self.shown_at = None;
    }

    /// Clears the message after five seconds without a newer error.
    pub fn clear_expired(&mut self, now: Instant) {
        if self
            .shown_at
            .is_some_and(|shown| now.duration_since(shown) >= ERROR_VISIBLE_FOR)
        {
            self.clear();
        }
    }

    /// The message currently shown, if any.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Draws the bar across `screen_width` pixels, when a message is set, with `font`.
    pub fn draw(&self, ui: &mut Ui, screen_width: f32, font: FontId) {
        let Some(first_line) = self.message.as_deref().and_then(|m| m.lines().next()) else {
            return;
        };
        ui.rect(0.0, 0.0, screen_width, BAR_HEIGHT, 0.0, rgba(BAR_HEX, 0.95));
        ui.text(
            font,
            PADDING,
            TEXT_TOP,
            TEXT_PX,
            rgba(0xff_ffff, 1.0),
            Align::Left,
            first_line,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_banner_shows_nothing() {
        let banner = ErrorBanner::default();
        assert_eq!(banner.message(), None);
    }

    #[test]
    fn show_replaces_and_clear_hides() {
        let now = Instant::now();
        let mut banner = ErrorBanner::default();
        banner.show("first", now);
        banner.show("second", now);
        assert_eq!(banner.message(), Some("second"));
        banner.clear();
        assert_eq!(banner.message(), None);
    }

    #[test]
    fn the_banner_expires_five_seconds_after_the_newest_error() {
        let start = Instant::now();
        let mut banner = ErrorBanner::default();
        banner.show("first", start);
        banner.show("newest", start + Duration::from_secs(4));
        banner.clear_expired(start + Duration::from_secs(8));
        assert_eq!(banner.message(), Some("newest"));
        banner.clear_expired(start + Duration::from_secs(9));
        assert_eq!(banner.message(), None);
    }
}
