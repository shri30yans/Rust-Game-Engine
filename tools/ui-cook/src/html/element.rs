//! The element being converted, with the helpers the content builders share. Each helper names the
//! element, so every error says where it happened.

use std::collections::BTreeMap;

use engine_ui::doc::{Color, TextAlign};
use markup5ever_rcdom::Handle;

use crate::css::Rule;
use crate::error::CookError;

/// One element of the page during conversion.
pub(super) struct Element<'a> {
    pub(super) handle: &'a Handle,
    pub(super) rules: &'a [Rule],
    /// Position in the page, such as `body/0/2`, for the errors of child elements.
    pub(super) path: &'a str,
    pub(super) tag: &'a str,
    pub(super) attrs: &'a BTreeMap<String, String>,
    /// The element name as shown in errors.
    pub(super) who: &'a str,
    pub(super) font_size: Option<f32>,
    pub(super) color: Option<Color>,
    pub(super) text_align: TextAlign,
}

impl Element<'_> {
    /// A required attribute.
    pub(super) fn attr(&self, name: &'static str) -> Result<&String, CookError> {
        self.attrs
            .get(name)
            .ok_or_else(|| CookError::MissingAttribute {
                element: self.who.to_owned(),
                attribute: name,
            })
    }

    /// A structure error: the element is in the right place for the subset but its content is not.
    pub(super) fn structure(&self, reason: &'static str) -> CookError {
        CookError::Structure {
            element: self.who.to_owned(),
            reason,
        }
    }

    /// A required property that the element's style did not set.
    pub(super) fn needs(
        &self,
        value: Option<f32>,
        property: &'static str,
    ) -> Result<f32, CookError> {
        value.ok_or_else(|| CookError::MissingProperty {
            element: self.who.to_owned(),
            property,
        })
    }

    /// The element's text colour, which every widget that draws text requires.
    pub(super) fn needs_colour(&self) -> Result<Color, CookError> {
        self.color.ok_or_else(|| CookError::MissingProperty {
            element: self.who.to_owned(),
            property: "color",
        })
    }
}
