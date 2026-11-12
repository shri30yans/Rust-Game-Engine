//! Widget-state deltas: the `state-hover-`, `state-focus-`, `state-pressed-` and
//! `state-disabled-` declarations, read into one [`StyleDelta`] per state.
//! Invariant: only the properties a state may change are accepted; any other is an error.

use engine_ui::doc::{Border, Color, StyleDelta};

use crate::css::Value;
use crate::error::CookError;

use super::props::{PropMap, Props};

/// Reads the declarations with `prefix` into one delta, or `None` when there are none.
pub(super) fn state_delta(p: &mut Props, prefix: &str) -> Result<Option<StyleDelta>, CookError> {
    let mut props = PropMap::new();
    let keys: Vec<String> = p
        .props
        .keys()
        .filter(|name| name.starts_with(prefix))
        .cloned()
        .collect();
    for key in keys {
        let Some(values) = p.props.remove(&key) else {
            continue;
        };
        let property = key.strip_prefix(prefix).unwrap_or(&key);
        if ![
            "background-color",
            "border",
            "opacity",
            "color",
            "transform",
        ]
        .contains(&property)
        {
            return Err(p.bad(
                &key,
                "a widget state property: background-color, border, opacity, color or scale",
            ));
        }
        let normalized = if property == "transform" {
            "scale"
        } else {
            property
        };
        props.insert(normalized.to_owned(), values);
    }
    if props.is_empty() {
        return Ok(None);
    }
    let mut state = Props {
        props,
        who: p.who.clone(),
    };
    let fill = state.color("background-color")?;
    let text_color = state.color("color")?;
    let opacity = state.number("opacity")?;
    let scale = match state.one("scale")? {
        None => None,
        Some(Value::Scale(value)) | Some(Value::Number(value)) => Some(value),
        Some(_) => return Err(state.bad("scale", "scale(<n>)")),
    };
    let border = if let Some(values) = state.take("border") {
        Some(match values.as_slice() {
            [Value::Px(width), Value::Ident(solid), Value::Color(hex)] if solid == "solid" => {
                Border {
                    width: *width,
                    color: Color::parse(hex)?,
                }
            }
            _ => return Err(state.bad("border", "`<n>px solid #colour`")),
        })
    } else {
        None
    };
    if let Some(name) = state.props.keys().next() {
        return Err(state.bad(name, "a supported widget state property"));
    }
    Ok(Some(StyleDelta {
        fill,
        background: None,
        border,
        opacity,
        text_color,
        scale,
    }))
}
