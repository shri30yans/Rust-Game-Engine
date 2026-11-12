//! Public draw, measurement and rectangle types.

use std::ops::Range;

use engine_core::{Rect, TouchRegion, TouchRegionId};

use crate::brush::GradientStop;
use crate::color::Srgb8;
use crate::doc::{Color, TextAlign};
use crate::focus::RowIndex;

/// The screen short side, in pixels, at which one logical unit is one pixel.
pub const REFERENCE_SHORT_SIDE: f32 = 720.0;

/// A colour ready to draw: sRGB plus opacity with the node's and its parents' opacity applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    /// The sRGB colour.
    pub rgb: Srgb8,
    /// Opacity in `[0, 1]`.
    pub alpha: f32,
}

impl Paint {
    pub(crate) fn of(color: Color, opacity: f32) -> Self {
        Self {
            rgb: color.rgb,
            alpha: color.alpha() * opacity,
        }
    }
}

/// A horizontal shear applied to the shapes and text that follow it until the next
/// [`Primitive::Skew`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skew {
    /// How far the top leans right per pixel of height (the tangent of the angle, flipped so
    /// that CSS `skewX(-10deg)` gives a positive value).
    pub lean: f32,
    /// The screen y that stays in place.
    pub pivot_y: f32,
}

/// One thing to draw.
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    /// Sets (or with `None`, clears) the shear for the primitives that follow. Pairs are
    /// emitted around a skewed node's subtree and restore the enclosing node's value.
    Skew(Option<Skew>),
    /// A filled, optionally rounded rectangle.
    Fill {
        /// Where.
        rect: Rect,
        /// Corner radius in pixels.
        radius: f32,
        /// Colour with opacity applied.
        color: Paint,
    },
    /// A rectangle outline.
    Outline {
        /// Where.
        rect: Rect,
        /// Corner radius in pixels.
        radius: f32,
        /// Line width in pixels.
        width: f32,
        /// Colour with opacity applied.
        color: Paint,
    },
    /// A circular ring.
    Ring {
        /// Centre x.
        cx: f32,
        /// Centre y.
        cy: f32,
        /// Outer radius.
        radius: f32,
        /// Line width.
        width: f32,
        /// Colour with opacity applied.
        color: Paint,
    },
    /// A filled circle.
    Disc {
        /// Centre x.
        cx: f32,
        /// Centre y.
        cy: f32,
        /// Radius.
        radius: f32,
        /// Colour with opacity applied.
        color: Paint,
    },
    /// A line of text; the characters are in [`DrawList::text`] at `range`.
    Text {
        /// The box the text sits in.
        rect: Rect,
        /// Glyph height in pixels.
        px: f32,
        /// Colour with opacity applied.
        color: Paint,
        /// Alignment inside `rect`.
        align: TextAlign,
        /// Byte range into [`DrawList::text`].
        range: Range<usize>,
    },
    /// A linear gradient fill inside `rect`.
    LinearGradient {
        /// Bounds in screen pixels.
        rect: Rect,
        /// Corner radius in pixels.
        radius: f32,
        /// Clockwise angle in degrees.
        angle_deg: f32,
        /// Packed stops; only the first `stop_count` entries are valid.
        stops: [GradientStop; 4],
        /// Number of valid entries in `stops`.
        stop_count: u8,
    },
}

/// Original rectangle of a visible touch control before viewport clipping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchRegionBounds {
    /// Stable identifier declared by the button node.
    pub id: TouchRegionId,
    /// The button's full painted rectangle in screen pixels.
    pub rect: Rect,
}

/// Laid out bounds of a menu widget, for focus navigation and pointer hit testing.
#[derive(Clone, Debug, PartialEq)]
pub struct WidgetRegion {
    /// Index into the document-order widget table.
    pub widget_index: usize,
    /// Repeated-list row for this widget, absent for static widgets.
    pub row: Option<RowIndex>,
    /// Widget bounds in screen pixels.
    pub rect: Rect,
    /// Inherited shear used to draw this widget, if any.
    pub skew: Option<Skew>,
    /// Whether the widget is disabled by its binding.
    pub disabled: bool,
}

/// The laid out box of one document node, including structural containers.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeRect {
    /// Stable source path from the cooked document.
    pub source: String,
    /// Box in physical screen pixels.
    pub rect: Rect,
    /// Whether the node participates in this layout.
    pub visible: bool,
    /// Whether the node draws text, a bar, or an interactive control.
    pub visible_content: bool,
}

/// Bounds and measured size for one visible text run.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRunBounds {
    /// Preorder index of the document node that produced the run.
    pub node: usize,
    /// Text box in screen pixels.
    pub rect: Rect,
    /// Immediate parent's box in screen pixels.
    pub parent: Rect,
    /// Measured text width after fitting.
    pub width: f32,
    /// Font height after fitting.
    pub px: f32,
}

/// Current interaction state supplied by the menu runtime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetVisualState {
    /// Pointer is hovering this widget.
    pub hovered: bool,
    /// Keyboard or pointer focus is on this widget.
    pub focused: bool,
    /// Touch or pointer is pressing this widget.
    pub pressed: bool,
    /// Widget is disabled.
    pub disabled: bool,
}

/// The output of one layout pass.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawList {
    pub(crate) prims: Vec<Primitive>,
    pub(crate) text: String,
    pub(crate) touch_regions: Vec<TouchRegion>,
    pub(crate) touch_bounds: Vec<TouchRegionBounds>,
    pub(crate) widgets: Vec<WidgetRegion>,
    pub(crate) text_runs: Vec<TextRunBounds>,
}

impl DrawList {
    /// The primitives, back to front.
    #[must_use]
    pub fn primitives(&self) -> &[Primitive] {
        &self.prims
    }

    /// Backing store for text primitives' ranges.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text of a text primitive, or `None` if its range is not valid for this list.
    #[must_use]
    pub fn text_of(&self, range: &Range<usize>) -> Option<&str> {
        self.text.get(range.clone())
    }

    /// The clipped touch areas of every visible button, in painter's order.
    #[must_use]
    pub fn touch_regions(&self) -> &[TouchRegion] {
        &self.touch_regions
    }

    /// Original bounds of every visible touch control, before viewport clipping.
    #[must_use]
    pub fn touch_bounds(&self) -> &[TouchRegionBounds] {
        &self.touch_bounds
    }

    /// The visible focusable widgets, in document order.
    #[must_use]
    pub fn widgets(&self) -> &[WidgetRegion] {
        &self.widgets
    }

    /// The visible text runs and their measured layout bounds.
    #[must_use]
    pub fn text_runs(&self) -> &[TextRunBounds] {
        &self.text_runs
    }

    pub(crate) fn clear(&mut self) {
        self.prims.clear();
        self.text.clear();
        self.touch_regions.clear();
        self.touch_bounds.clear();
        self.widgets.clear();
        self.text_runs.clear();
    }
}

/// Measures text so auto-sized text nodes get their natural width.
pub trait TextMeasure {
    /// Identity of font metrics. Change it whenever text widths may change.
    fn metrics_key(&self) -> MetricsKey;

    /// Width in pixels of `text` drawn at glyph height `px`.
    fn width(&mut self, px: f32, text: &str) -> f32;
}

/// Stable identity for a text measurer's font metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MetricsKey(pub u64);
