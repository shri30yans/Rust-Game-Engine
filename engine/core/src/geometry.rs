//! Screen-space geometry shared by layout and input.
//!
//! Invariants: [`ScreenRect`] can only be created by clipping a finite positive [`Rect`] to a
//! finite positive [`Viewport`]. It therefore never describes pixels outside its viewport.

/// An axis-aligned rectangle in screen pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

/// A finite, positive screen viewport in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    width: f32,
    height: f32,
}

impl Viewport {
    /// Builds a viewport when both dimensions are finite and positive.
    #[must_use]
    pub fn new(width: f32, height: f32) -> Option<Self> {
        (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
            .then_some(Self { width, height })
    }

    /// Clips `rect` to this viewport, returning `None` when it has no visible area.
    #[must_use]
    pub fn clip(self, rect: Rect) -> Option<ScreenRect> {
        if !(rect.x.is_finite()
            && rect.y.is_finite()
            && rect.w.is_finite()
            && rect.h.is_finite()
            && rect.w > 0.0
            && rect.h > 0.0)
        {
            return None;
        }
        let right = rect.x + rect.w;
        let bottom = rect.y + rect.h;
        if !(right.is_finite() && bottom.is_finite()) {
            return None;
        }
        let left = rect.x.max(0.0);
        let top = rect.y.max(0.0);
        let right = right.min(self.width);
        let bottom = bottom.min(self.height);
        (right > left && bottom > top).then_some(ScreenRect(Rect {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        }))
    }
}

/// A positive pixel rectangle guaranteed to be inside the viewport that created it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenRect(Rect);

impl ScreenRect {
    /// Returns the clipped rectangle in pixels.
    #[must_use]
    pub const fn rect(self) -> Rect {
        self.0
    }

    /// Whether the pixel lies in this rectangle, including its edges.
    #[must_use]
    pub fn contains(self, x: f32, y: f32) -> bool {
        let rect = self.0;
        x >= rect.x && x <= rect.x + rect.w && y >= rect.y && y <= rect.y + rect.h
    }
}

/// Stable identifier of a touch control declared by a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TouchRegionId(pub u8);

/// The hit shape for a [`TouchRegion`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchShape {
    /// The complete clipped rectangle is interactive.
    Rect,
    /// A circle inscribed in the clipped rectangle is interactive.
    Circle,
}

/// A visible, clipped touch target emitted by UI drawing and retained by input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchRegion {
    /// The game's stable control identifier.
    pub id: TouchRegionId,
    /// The visible part of the drawn control in physical screen pixels.
    pub rect: ScreenRect,
    /// The visible control's hit shape.
    pub shape: TouchShape,
    /// Whether dragging the held control also turns the view.
    pub drag_look: bool,
}

impl TouchRegion {
    /// Whether this region contains a pixel coordinate.
    #[must_use]
    pub fn contains(self, x: f32, y: f32) -> bool {
        if !self.rect.contains(x, y) {
            return false;
        }
        match self.shape {
            TouchShape::Rect => true,
            TouchShape::Circle => {
                let rect = self.rect.rect();
                let radius = rect.w.min(rect.h) * 0.5;
                let dx = x - (rect.x + rect.w * 0.5);
                let dy = y - (rect.y + rect.h * 0.5);
                dx * dx + dy * dy <= radius * radius
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Rect, Viewport};

    #[test]
    fn clip_keeps_an_inside_rectangle() {
        let viewport = Viewport::new(100.0, 80.0).unwrap();
        assert_eq!(
            viewport
                .clip(Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 30.0,
                    h: 40.0
                })
                .unwrap()
                .rect(),
            Rect {
                x: 10.0,
                y: 20.0,
                w: 30.0,
                h: 40.0
            }
        );
    }

    #[test]
    fn clip_trims_a_partly_outside_rectangle() {
        let viewport = Viewport::new(100.0, 80.0).unwrap();
        assert_eq!(
            viewport
                .clip(Rect {
                    x: -10.0,
                    y: 60.0,
                    w: 30.0,
                    h: 30.0
                })
                .unwrap()
                .rect(),
            Rect {
                x: 0.0,
                y: 60.0,
                w: 20.0,
                h: 20.0
            }
        );
    }

    #[test]
    fn clip_drops_a_wholly_outside_rectangle() {
        let viewport = Viewport::new(100.0, 80.0).unwrap();
        assert_eq!(
            viewport.clip(Rect {
                x: 110.0,
                y: 0.0,
                w: 10.0,
                h: 10.0
            }),
            None
        );
    }
}
