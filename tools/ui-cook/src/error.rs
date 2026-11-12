//! Errors from compiling HTML and CSS into a UI document. Every unsupported construct is an
//! error that names what and where; nothing is skipped silently.

use thiserror::Error;

/// Why a page could not be compiled.
#[derive(Debug, Error)]
pub enum CookError {
    /// The HTML could not be read.
    #[error("html: {0}")]
    Html(String),
    /// The CSS is malformed.
    #[error("css: {reason}")]
    Css {
        /// What the parser found.
        reason: String,
    },
    /// A selector other than `tag`, `.class` or `#id`, or a list of those joined by commas.
    #[error("unsupported selector: {0}")]
    UnsupportedSelector(String),
    /// An element outside the subset.
    #[error("unsupported element <{0}>")]
    UnsupportedElement(String),
    /// An attribute outside the subset.
    #[error("{element}: unsupported attribute `{attribute}`")]
    UnsupportedAttribute {
        /// Where.
        element: String,
        /// The attribute.
        attribute: String,
    },
    /// A CSS property outside the subset, or one with no meaning on this element.
    #[error("{element}: unsupported property `{property}`")]
    UnsupportedProperty {
        /// Where.
        element: String,
        /// The property.
        property: String,
    },
    /// A value the property does not accept.
    #[error("{element}: bad value for `{property}`: {reason}")]
    BadValue {
        /// Where.
        element: String,
        /// The property.
        property: String,
        /// What is wrong.
        reason: &'static str,
    },
    /// A property this element needs and does not have.
    #[error("{element}: missing property `{property}`")]
    MissingProperty {
        /// Where.
        element: String,
        /// The property.
        property: &'static str,
    },
    /// An attribute this element needs and does not have.
    #[error("{element}: missing attribute `{attribute}`")]
    MissingAttribute {
        /// Where.
        element: String,
        /// The attribute.
        attribute: &'static str,
    },
    /// Loose text in a container, or an element inside a text element.
    #[error("{element}: {reason}")]
    Structure {
        /// Where.
        element: String,
        /// What is wrong.
        reason: &'static str,
    },
    /// The page has no `<body>` content to make a document from.
    #[error("the page has no body")]
    NoBody,
    /// The compiled document is not valid.
    #[error("document: {0}")]
    Ui(#[from] engine_ui::UiError),
}
