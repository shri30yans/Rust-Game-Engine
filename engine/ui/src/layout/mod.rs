//! Retained UI layout, measurement, rectangle reporting and drawing.
//!
//! CSS block and flex nodes share one retained Taffy tree. Repeat row slots and source paths are
//! built at load; warmed frames reuse the layout and draw buffers.

mod data;
mod internal;

pub use data::{
    DrawList, MetricsKey, NodeRect, Paint, Primitive, REFERENCE_SHORT_SIDE, Skew, TextMeasure,
    TextRunBounds, TouchRegionBounds, WidgetRegion, WidgetVisualState,
};

pub use internal::UiLayout;
