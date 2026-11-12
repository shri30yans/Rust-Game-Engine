//! RON document loading, validation entry point and serialisation.

use serde::{Deserialize, Serialize};

use super::FORMAT_VERSION;
use super::content::{BackRule, Node, ScreenDef};
use super::validation::{Seen, validate};
use crate::error::UiError;

/// A validated UI document.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    root: Node,
    screen: Option<ScreenDef>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct File<N> {
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    screen: Option<ScreenDef>,
    root: N,
}

impl Document {
    /// Reads and validates a document.
    ///
    /// # Errors
    ///
    /// Returns [`UiError::DocParse`] for syntax errors, unknown fields and malformed colours or
    /// templates, and [`UiError::DocInvalid`] for a value that is out of range, a duplicate id, a
    /// wrong `version`, or children under a node that cannot have them.
    pub fn from_ron(source: &str) -> Result<Self, UiError> {
        let file: File<Node> =
            ron::from_str(source).map_err(|e| UiError::DocParse(e.to_string()))?;
        if file.version != 1 && file.version != FORMAT_VERSION {
            return Err(super::validation::invalid(
                "root",
                "unsupported document version",
            ));
        }
        Self::with_screen(file.root, file.screen)
    }

    /// Validates `root` and wraps it as a document.
    ///
    /// # Errors
    ///
    /// Returns [`UiError::DocInvalid`] as [`Document::from_ron`] does.
    pub fn new(root: Node) -> Result<Self, UiError> {
        Self::with_screen(root, None)
    }

    /// Validates a document with screen-level navigation metadata.
    pub fn with_screen(root: Node, screen: Option<ScreenDef>) -> Result<Self, UiError> {
        let mut seen = Seen::default();
        validate(&root, "root", &mut seen)?;
        if let Some(screen) = &screen {
            if let Some(name) = &screen.default_focus
                && !seen.widgets.contains(name)
            {
                return Err(UiError::UnknownDefaultFocus(name.clone()));
            }
            if let BackRule::Action(name) = &screen.back
                && !seen.actions.contains(name)
            {
                return Err(UiError::UnknownBackAction(name.clone()));
            }
        }
        Ok(Self { root, screen })
    }

    /// The document as RON text that [`Document::from_ron`] reads back to an equal document.
    ///
    /// # Errors
    ///
    /// Returns [`UiError::DocParse`] if serialisation fails.
    pub fn to_ron(&self) -> Result<String, UiError> {
        let file = File {
            version: FORMAT_VERSION,
            screen: self.screen.clone(),
            root: &self.root,
        };
        ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
            .map_err(|e| UiError::DocParse(e.to_string()))
    }

    /// The root node.
    #[must_use]
    pub const fn root(&self) -> &Node {
        &self.root
    }

    /// Screen-level navigation metadata.
    #[must_use]
    pub fn screen(&self) -> Option<&ScreenDef> {
        self.screen.as_ref()
    }
}
