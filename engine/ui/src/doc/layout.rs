//! Document layout values and algorithms.

use serde::{Deserialize, Serialize};

/// A length.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
pub enum Len {
    /// Sized by content (text) or by the layout (stretch, grow).
    #[default]
    Auto,
    /// Logical units, scaled to the screen.
    Px(f32),
    /// A fraction `0..=1` of the parent's size on the same axis.
    Percent(f32),
    /// A fraction `0..=1` of the screen's short side, so a square stays square in any orientation.
    Short(f32),
}

/// Where a node sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
pub enum Position {
    /// In the parent's flow.
    #[default]
    Flow,
    /// Pinned to the parent's edges. Give two opposite edges to stretch, or one edge per axis plus a
    /// size to anchor a corner.
    Absolute {
        /// Distance from the left edge.
        #[serde(default)]
        left: Option<Len>,
        /// Distance from the top edge.
        #[serde(default)]
        top: Option<Len>,
        /// Distance from the right edge.
        #[serde(default)]
        right: Option<Len>,
        /// Distance from the bottom edge.
        #[serde(default)]
        bottom: Option<Len>,
    },
}

/// The direction children flow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum Direction {
    /// Left to right.
    #[default]
    Row,
    /// Top to bottom.
    Column,
}

/// The algorithm used to lay out a node's children.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum Display {
    /// CSS block layout, used when no display is specified.
    #[default]
    Block,
    /// CSS flexbox layout.
    Flex,
}

/// Placement of children across the flow direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum Align {
    /// At the start.
    Start,
    /// Centred.
    Center,
    /// At the end.
    End,
    /// Stretched to fill.
    #[default]
    Stretch,
}

/// Placement of children along the flow direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum Justify {
    /// Packed at the start.
    #[default]
    Start,
    /// Packed in the middle.
    Center,
    /// Packed at the end.
    End,
    /// Spread with the free space between children.
    SpaceBetween,
    /// Spread with the free space around children.
    SpaceAround,
}

/// How a node is sized and placed.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutDef {
    /// The layout algorithm; the HTML cooker supplies CSS defaults explicitly.
    pub display: Display,
    /// Flow or pinned.
    pub position: Position,
    /// Width.
    pub width: Len,
    /// Height.
    pub height: Len,
    /// The direction children flow.
    pub direction: Direction,
    /// Space between children, in logical units.
    pub gap: f32,
    /// Space inside the node's edge, in logical units.
    pub padding: f32,
    /// Space outside the node's edge, in logical units.
    pub margin: f32,
    /// Placement of children across the flow.
    pub align: Align,
    /// Placement of children along the flow.
    pub justify: Justify,
    /// Share of the parent's free space this node takes along the flow.
    pub grow: f32,
}
