//! Visual style values for document nodes.

use serde::{Deserialize, Serialize};

use crate::brush::Background;
use crate::color::Color;

/// An outline.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Border {
    /// Line width in logical units.
    pub width: f32,
    /// Line colour.
    pub color: Color,
}

/// How a node looks, apart from its content.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StyleDef {
    /// Background colour.
    pub fill: Option<Color>,
    /// Background brush: nothing, a solid colour or a linear gradient.
    #[serde(default)]
    pub background: Background,
    /// Corner radius in logical units.
    pub radius: f32,
    /// Outline.
    pub border: Option<Border>,
    /// Opacity of the node and everything inside it, `0..=1`.
    pub opacity: f32,
    /// Horizontal skew of the node and everything inside it, in degrees with CSS `skewX`'s sign
    /// (negative leans the top to the right), about the node's vertical centre. Applies to
    /// fills, outlines, bars and text; buttons, rings and discs stay upright so touch areas
    /// match what is drawn. The innermost skewed node wins.
    pub skew: f32,
    /// Base text colour used by text and menu widgets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<Color>,
    /// Visual overrides for interactive widget states.
    #[serde(default, skip_serializing_if = "StateStyles::is_empty")]
    pub states: StateStyles,
}

/// State-specific visual overrides for focusable widgets.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StateStyles {
    /// Pointer hover override.
    pub hover: Option<StyleDelta>,
    /// Keyboard or pointer focus override.
    pub focus: Option<StyleDelta>,
    /// Pressed or activated override.
    pub pressed: Option<StyleDelta>,
    /// Disabled override, which takes precedence over all other states.
    pub disabled: Option<StyleDelta>,
}

impl StateStyles {
    fn is_empty(&self) -> bool {
        self.hover.is_none()
            && self.focus.is_none()
            && self.pressed.is_none()
            && self.disabled.is_none()
    }
}

/// Optional visual properties that replace the base style for a widget state.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StyleDelta {
    /// Replacement background colour.
    pub fill: Option<Color>,
    /// Replacement background brush.
    #[serde(default)]
    pub background: Option<Background>,
    /// Replacement outline.
    pub border: Option<Border>,
    /// Replacement opacity.
    pub opacity: Option<f32>,
    /// Replacement text colour.
    pub text_color: Option<Color>,
    /// Uniform scale around the node centre.
    pub scale: Option<f32>,
}

impl Default for StyleDef {
    fn default() -> Self {
        Self {
            fill: None,
            background: Background::None,
            radius: 0.0,
            border: None,
            opacity: 1.0,
            skew: 0.0,
            text_color: None,
            states: StateStyles::default(),
        }
    }
}

impl StyleDef {
    /// Resolved background: explicit brush, else legacy flat fill.
    #[must_use]
    pub fn resolved_background(&self) -> Background {
        if !matches!(self.background, Background::None) {
            return self.background.clone();
        }
        self.fill.map_or(Background::None, Background::Solid)
    }
}
