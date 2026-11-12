//! Renderer-independent UI: theme tokens, the screen layer stack, spatial focus navigation, and a
//! declarative UI document with flexbox layout that turns into a flat list of draw primitives.
//!
//! Invariants: token names are game data; this crate only defines the file shapes. A [`Document`]
//! that exists is valid. `FocusNav::navigate` and `ScreenStack` queries allocate nothing;
//! [`UiLayout::frame`] retains its document, tree and output buffers between changed frames.
#![forbid(unsafe_code)]

pub mod bindings;
pub mod brush;
pub mod checks;
pub mod color;
pub mod doc;
pub mod error;
pub mod focus;
pub mod layout;
pub mod menus;
pub mod stack;
pub mod template;
pub mod theme;

pub use bindings::{Bindings, ListBinding, ListRecord, Value};
pub use brush::{Background, GradientStop, check_gradient};
pub use checks::{check_interactive_bounds, check_text_overflow};
pub use color::{Color, Srgb8};
pub use doc::{
    BackRule, Display, Document, FORMAT_VERSION, ScreenDef, StateStyles, StyleDelta, TextFit,
    TextOverflow,
};
pub use engine_core::{Rect, ScreenRect, TouchRegion, TouchRegionId, TouchShape, Viewport};
pub use error::UiError;
pub use focus::{FocusNav, FocusOverride, FocusRect, NavInput, RowIndex, WidgetId};
pub use layout::{
    DrawList, MetricsKey, NodeRect, Paint, Primitive, Skew, TextMeasure, TextRunBounds,
    TouchRegionBounds, UiLayout, WidgetRegion, WidgetVisualState,
};
pub use menus::{
    MenuDraw, MenuInput, MenuMotion, MenuTouch, MenuTouchPhase, Menus, PointerInput, Transition,
    UiEvent, cubic_bezier,
};
pub use stack::{Layer, ScreenId, ScreenStack};
pub use template::Template;
pub use theme::{
    ColorId, ContrastRule, FontFile, GlyphId, MotionEntry, MotionId, RadiusId, ScrimFile, SizeId,
    SpaceId, Theme, ThemeSerial, TouchFile, TypeId, TypeScaleEntry,
};
