//! Validated UI document types and their RON boundary.
//!
//! A document is validated once when it is constructed; values that reach layout satisfy the
//! document invariants. Public types remain available through `engine_ui::doc`.

mod content;
mod document;
mod layout;
mod style;
mod validation;

#[cfg(test)]
mod tests;

pub use crate::color::Color;
pub use content::{BackRule, Content, Node, ScreenDef, TextAlign, TextFit, TextOverflow};
pub use document::Document;
pub use layout::{Align, Direction, Display, Justify, LayoutDef, Len, Position};
pub use style::{Border, StateStyles, StyleDef, StyleDelta};

/// The only document version this build reads.
pub const FORMAT_VERSION: u32 = 3;
