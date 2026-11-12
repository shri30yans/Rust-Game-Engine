//! Content and node definitions used by validated documents.

use serde::{Deserialize, Serialize};

use super::layout::LayoutDef;
use super::style::StyleDef;
use crate::color::Color;
use crate::template::Template;

/// Alignment of text inside its node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum TextAlign {
    /// At the left edge.
    #[default]
    Start,
    /// Centred.
    Center,
    /// At the right edge.
    End,
}

/// How text reduces its font size when its node has a fixed width.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub enum TextFit {
    /// Keep the declared font size.
    None,
    /// Shrink down to a fraction of the declared font size.
    Shrink {
        /// Smallest allowed fraction, in `(0, 1]`.
        minimum: f32,
    },
}

impl Default for TextFit {
    fn default() -> Self {
        Self::Shrink { minimum: 0.75 }
    }
}

/// How text is shortened if it still exceeds its node after fitting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum TextOverflow {
    /// Trim at a character boundary and append an ellipsis.
    #[default]
    Ellipsis,
}

/// What a node shows.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub enum Content {
    /// Nothing; the node is a container or a plain box.
    #[default]
    None,
    /// A line of text.
    Text {
        /// The text, with `{name}` slots.
        text: Template,
        /// Text height in logical units.
        size: f32,
        /// Text colour.
        color: Color,
        /// Alignment inside the node.
        #[serde(default)]
        align: TextAlign,
    },
    /// A filled bar: the node's own fill is the track, and this draws the filled part.
    Bar {
        /// The number variable (`0..=1`) that sets how much is filled.
        value: String,
        /// Colour of the filled part.
        color: Color,
    },
    /// A round touch button inscribed in the node's box.
    Button {
        /// The id the game's input code reads. Unique in the document.
        id: u8,
        /// The text on the button.
        label: String,
        /// Colour of the ring and label.
        color: Color,
        /// The flag variable that is true while the button is held.
        held: String,
        /// Whether a finger held on the button also steers the look (fire that aims).
        #[serde(default)]
        drag_look: bool,
    },
    /// A menu action button.
    Action {
        /// Unique widget id.
        id: String,
        /// Action label.
        label: Template,
        /// Optional disabled flag binding.
        #[serde(default)]
        disabled: Option<String>,
    },
    /// A boolean toggle.
    Toggle {
        /// Unique widget id.
        id: String,
        /// Toggle label.
        label: Template,
        /// Boolean value binding.
        value: String,
        /// Optional disabled flag binding.
        #[serde(default)]
        disabled: Option<String>,
    },
    /// A bounded numeric slider.
    Slider {
        /// Unique widget id.
        id: String,
        /// Slider label.
        label: Template,
        /// Numeric value binding.
        value: String,
        /// Inclusive lower bound.
        min: f32,
        /// Inclusive upper bound.
        max: f32,
        /// Positive adjustment step.
        step: f32,
        /// Optional disabled flag binding.
        #[serde(default)]
        disabled: Option<String>,
    },
    /// A left/right choice bound to a list of strings.
    Choice {
        /// Unique widget id.
        id: String,
        /// Choice label.
        label: Template,
        /// Options list binding.
        options: String,
        /// Selected zero-based index binding.
        selected: String,
        /// Optional disabled flag binding.
        #[serde(default)]
        disabled: Option<String>,
    },
    /// A tab strip bound to a list of strings.
    Tabs {
        /// Unique widget id.
        id: String,
        /// Tab list binding.
        tabs: String,
        /// Selected zero-based index binding.
        selected: String,
        /// Optional disabled flag binding.
        #[serde(default)]
        disabled: Option<String>,
    },
    /// A repeated template, expanded from a list binding up to its validated capacity.
    Repeat {
        /// Source list binding.
        source: String,
        /// Maximum row count, from 1 through 1024.
        max: u16,
        /// Per-row template.
        template: Box<Node>,
    },
}

/// Back behaviour declared by a screen.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub enum BackRule {
    /// Remove this screen from its layer.
    #[default]
    Pop,
    /// Emit the named action instead of popping.
    Action(String),
}

/// Screen-level focus and Back behaviour.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScreenDef {
    /// Initial focus target; absent selects the first widget in document order.
    pub default_focus: Option<String>,
    /// Behaviour for the Back input.
    pub back: BackRule,
}

/// A node of the document.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Node {
    /// Stable source path used to diagnose layout errors and compare browser geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// A name for the node, for errors and tools. Unique in the document when given.
    pub id: Option<String>,
    /// Size and placement.
    pub layout: LayoutDef,
    /// Appearance.
    pub style: StyleDef,
    /// Font fitting rule for the node's text content.
    #[serde(default, skip_serializing_if = "text_fit_is_default")]
    pub fit: TextFit,
    /// Overflow marker for text that remains wider than the node.
    #[serde(default, skip_serializing_if = "text_overflow_is_default")]
    pub text_overflow: TextOverflow,
    /// Shown only while this flag variable is true; a leading `!` shows it while false.
    pub show_if: Option<String>,
    /// What the node shows.
    pub content: Content,
    /// Children, drawn in order (later on top).
    pub children: Vec<Node>,
}

fn text_fit_is_default(value: &TextFit) -> bool {
    *value == TextFit::default()
}

fn text_overflow_is_default(value: &TextOverflow) -> bool {
    *value == TextOverflow::default()
}
