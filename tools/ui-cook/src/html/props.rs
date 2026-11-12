//! Typed access to one element's declared properties. Each accessor removes the property it reads,
//! so whatever is left over at the end is a property the cooker does not support.
//! Invariant: a value of the wrong kind is a [`CookError::BadValue`] naming the element and property.

use std::collections::BTreeMap;

use engine_ui::doc::{Color, Len, TextFit, TextOverflow};
use engine_ui::{Background, GradientStop, check_gradient};

use crate::css::Value;
use crate::error::CookError;

/// Declared values by property name, as the cascade produces them.
pub(super) type PropMap = BTreeMap<String, Vec<Value>>;

/// An element's remaining properties, with the element name used in error messages.
pub(super) struct Props {
    pub(super) props: PropMap,
    pub(super) who: String,
}

impl Props {
    pub(super) fn bad(&self, property: &str, reason: &'static str) -> CookError {
        CookError::BadValue {
            element: self.who.clone(),
            property: property.to_owned(),
            reason,
        }
    }

    pub(super) fn take(&mut self, name: &str) -> Option<Vec<Value>> {
        self.props.remove(name)
    }

    pub(super) fn one(&mut self, name: &str) -> Result<Option<Value>, CookError> {
        match self.take(name) {
            None => Ok(None),
            Some(mut v) if v.len() == 1 => Ok(v.pop()),
            Some(_) => Err(self.bad(name, "exactly one value")),
        }
    }

    pub(super) fn px(&mut self, name: &str) -> Result<Option<f32>, CookError> {
        match self.one(name)? {
            None => Ok(None),
            Some(Value::Px(v)) if v >= 0.0 => Ok(Some(v)),
            Some(Value::Number(0.0)) => Ok(Some(0.0)),
            Some(_) => Err(self.bad(name, "a non-negative px length")),
        }
    }

    pub(super) fn len(&mut self, name: &str) -> Result<Option<Len>, CookError> {
        match self.one(name)? {
            None => Ok(None),
            Some(Value::Px(v)) => Ok(Some(Len::Px(v))),
            Some(Value::Vmin(v)) => Ok(Some(Len::Short(v / 100.0))),
            Some(Value::Percent(v)) => Ok(Some(Len::Percent(v))),
            Some(Value::Number(0.0)) => Ok(Some(Len::Px(0.0))),
            Some(Value::Ident(i)) if i == "auto" => Ok(Some(Len::Auto)),
            Some(_) => Err(self.bad(name, "px, vmin, % or auto")),
        }
    }

    pub(super) fn color(&mut self, name: &str) -> Result<Option<Color>, CookError> {
        match self.one(name)? {
            None => Ok(None),
            Some(Value::Color(hex)) => Ok(Some(Color::parse(&hex)?)),
            Some(_) => Err(self.bad(name, "a #rrggbb or #rrggbbaa colour")),
        }
    }

    pub(super) fn keyword(&mut self, name: &str) -> Result<Option<String>, CookError> {
        match self.one(name)? {
            None => Ok(None),
            Some(Value::Ident(i)) => Ok(Some(i)),
            Some(_) => Err(self.bad(name, "a keyword")),
        }
    }

    pub(super) fn number(&mut self, name: &str) -> Result<Option<f32>, CookError> {
        match self.one(name)? {
            None => Ok(None),
            Some(Value::Number(v)) => Ok(Some(v)),
            Some(_) => Err(self.bad(name, "a number")),
        }
    }

    pub(super) fn skew(&mut self) -> Result<f32, CookError> {
        match self.one("transform")? {
            None => Ok(0.0),
            Some(Value::SkewX(degrees)) => Ok(degrees),
            Some(_) => Err(self.bad("transform", "skewX(<n>deg)")),
        }
    }

    /// The `background` property: a linear gradient, or nothing.
    pub(super) fn background(&mut self) -> Result<Background, CookError> {
        match self.one("background")? {
            None => Ok(Background::None),
            Some(Value::Ident(value)) if value == "none" => Ok(Background::None),
            Some(Value::Gradient { angle_deg, stops }) => {
                let mut built = Vec::with_capacity(stops.len());
                for (hex, pos) in stops {
                    let color = Color::parse(&hex).map_err(|_| {
                        self.bad("background", "a #rrggbb or #rrggbbaa colour stop")
                    })?;
                    built.push(GradientStop { pos, color });
                }
                check_gradient(angle_deg, &built)
                    .map_err(|reason| self.bad("background", reason))?;
                Ok(Background::LinearGradient {
                    angle_deg,
                    stops: built,
                })
            }
            Some(_) => Err(self.bad(
                "background",
                "linear-gradient(<angle>deg, <#colour> <n>%, ...) with two to four stops",
            )),
        }
    }

    pub(super) fn fit(&mut self) -> Result<TextFit, CookError> {
        match self.take("fit").as_deref() {
            None => Ok(TextFit::default()),
            Some([Value::Ident(value)]) if value == "none" => Ok(TextFit::None),
            Some([Value::Ident(value)]) if value == "shrink" => {
                Ok(TextFit::Shrink { minimum: 0.75 })
            }
            Some([Value::Ident(value), Value::Percent(minimum)]) if value == "shrink" => {
                Ok(TextFit::Shrink { minimum: *minimum })
            }
            Some(_) => Err(self.bad("fit", "none, shrink, or shrink <minimum-percent>")),
        }
    }

    pub(super) fn text_overflow(&mut self) -> Result<TextOverflow, CookError> {
        match self.keyword("text-overflow")?.as_deref() {
            None | Some("ellipsis") => Ok(TextOverflow::Ellipsis),
            Some(_) => Err(self.bad("text-overflow", "ellipsis")),
        }
    }
}
