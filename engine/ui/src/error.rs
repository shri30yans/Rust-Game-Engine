//! Errors returned by the UI model (`Theme` load, screen stack, focus).

use thiserror::Error;

/// Errors produced by `engine-ui`.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum UiError {
    /// The theme file could not be parsed or contained an unknown top-level section.
    #[error("theme parse failed: {0}")]
    Parse(String),
    /// A token name was not found in the named group at load or resolve time.
    #[error("unknown token {name:?} in group {group:?}")]
    UnknownToken {
        /// Token category (`colors`, `space`, …).
        group: &'static str,
        /// The missing name.
        name: String,
    },
    /// A colour string was not valid `#RRGGBB`.
    #[error("invalid colour {0}")]
    InvalidColor(String),
    /// A `contrast_rules` entry failed WCAG validation.
    #[error("contrast violation: fg {fg:?} on bg {bg:?} is {ratio:.2} (minimum {min:.2})")]
    ContrastViolation {
        /// Foreground colour token name.
        fg: String,
        /// Background colour token name.
        bg: String,
        /// Measured contrast ratio.
        ratio: f32,
        /// Required minimum ratio from the theme file.
        min: f32,
    },
    /// A screen stack had no screen to pop.
    #[error("screen stack is empty")]
    EmptyStack,
    /// A screen or widget identifier counter cannot allocate another value.
    #[error("UI identifier space exhausted")]
    IdExhausted,
    /// A bound list exceeds the repeat capacity declared in the document.
    #[error("list binding {name:?} has {actual} rows; repeat capacity is {max}")]
    RepeatTooLong {
        /// List binding name.
        name: String,
        /// Rows supplied by the game.
        actual: usize,
        /// Maximum declared by the document.
        max: u16,
    },
    /// Screen default focus names no focusable widget.
    #[error("screen default focus {0:?} does not name a focusable widget")]
    UnknownDefaultFocus(String),
    /// Screen Back action names no action widget.
    #[error("screen Back action {0:?} does not name an Action widget")]
    UnknownBackAction(String),
    /// A choice or tabs list cannot be represented by the `u16` event index.
    #[error("list binding {0:?} has more than 65,535 selectable entries")]
    ChoiceTooLarge(String),
    /// A token id was produced by a different theme instance.
    #[error("token id belongs to another theme")]
    ForeignId,
    /// A token id index does not exist in this theme (corrupt or forged id).
    #[error("token id out of range for this theme")]
    TokenOutOfRange,
    /// A UI document could not be parsed (syntax, unknown field, or a malformed value).
    #[error("ui document parse failed: {0}")]
    DocParse(String),
    /// A UI document parsed but a value is out of range or inconsistent.
    #[error("ui document invalid at {node}: {reason}")]
    DocInvalid {
        /// Path of the offending node, such as `root/2/0`.
        node: String,
        /// What is wrong, in plain words.
        reason: &'static str,
    },
    /// A node reads a variable that the game did not bind.
    #[error("ui node {node} reads unbound variable {name:?}")]
    UnboundVariable {
        /// Path of the node.
        node: String,
        /// The missing variable.
        name: String,
    },
    /// A node reads a variable that holds a different kind of value than the node needs.
    #[error("ui node {node} reads variable {name:?} as {expected}")]
    BindingType {
        /// Path of the node.
        node: String,
        /// The variable.
        name: String,
        /// The kind the node needs (`number`, `flag`).
        expected: &'static str,
    },
    /// The layout engine rejected the tree.
    #[error("ui layout failed: {0}")]
    Layout(String),
    /// A visible text run, bar, or widget has no laid out area.
    #[error("visible UI content has zero size at {node}")]
    ZeroSizeContent {
        /// Source path of the node with zero area.
        node: String,
    },
    /// A text run exceeded its measured width or parent bounds.
    #[error("text overflow at UI node {node}")]
    TextOverflow {
        /// Path of the offending node.
        node: String,
    },
    /// A visible interactive rectangle extends outside the supplied safe viewport.
    #[error("interactive {kind} {id} lies outside the safe viewport")]
    InteractiveOutsideSafeArea {
        /// Whether this is a touch control or menu widget.
        kind: &'static str,
        /// The document touch id or widget table index.
        id: usize,
    },
    /// Writing a formatted number into the text buffer failed.
    #[error("formatting failed")]
    Format(#[from] std::fmt::Error),
}
