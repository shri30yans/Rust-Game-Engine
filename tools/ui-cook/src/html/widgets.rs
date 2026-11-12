//! Builders for the interactive widgets: tab lists, buttons, inputs (checkbox and range) and
//! selects. Invariant: each widget requires its attributes up front, so a missing one is named
//! before any value is read from it.

use engine_ui::Template;
use engine_ui::doc::{Content, Node};

use crate::error::CookError;

use super::dom::text_of;
use super::element::Element;
use super::props::Props;

/// A `div role="tablist"`: the selected tab is bound, the tab labels come from `data-options`.
pub(super) fn tabs(el: &Element<'_>, node: &mut Node) -> Result<(), CookError> {
    let id = el.attr("id")?.clone();
    let selected = el.attr("data-bind")?.clone();
    let tabs = el.attr("data-options")?.clone();
    node.content = Content::Tabs {
        id,
        tabs,
        selected,
        disabled: el.attrs.get("data-disabled").cloned(),
    };
    node.style.text_color = el.color;
    Ok(())
}

/// A `button`: either an action (`data-action`) or a touch button with a numeric `data-id`.
pub(super) fn button(el: &Element<'_>, p: &Props, node: &mut Node) -> Result<(), CookError> {
    if let Some(action) = el.attrs.get("data-action") {
        node.style.text_color = el.color;
        node.content = Content::Action {
            id: action.clone(),
            label: Template::parse(text_of(el.handle).trim())?,
            disabled: el.attrs.get("data-disabled").cloned(),
        };
        return Ok(());
    }
    let id = el
        .attr("data-id")?
        .parse::<u8>()
        .map_err(|_| p.bad("data-id", "a number 0 to 255"))?;
    let label = text_of(el.handle).trim().to_owned();
    let color = el.needs_colour()?;
    node.content = Content::Button {
        id,
        label,
        color,
        held: el.attr("data-held")?.clone(),
        drag_look: el.attrs.contains_key("data-drag-look"),
    };
    Ok(())
}

/// An `input`: `type="checkbox"` is a toggle, `type="range"` a slider.
pub(super) fn input(el: &Element<'_>, p: &Props, node: &mut Node) -> Result<(), CookError> {
    let widget_id = el.attr("id")?.clone();
    let label = Template::parse(el.attrs.get("aria-label").map_or("", String::as_str))?;
    let binding = el.attr("data-bind")?.clone();
    let disabled = el.attrs.get("data-disabled").cloned();
    node.content = match el.attrs.get("type").map(String::as_str) {
        Some("checkbox") => Content::Toggle {
            id: widget_id,
            label,
            value: binding,
            disabled,
        },
        Some("range") => {
            let parse = |name: &'static str| -> Result<f32, CookError> {
                el.attr(name)?
                    .parse()
                    .map_err(|_| p.bad(name, "a finite number"))
            };
            Content::Slider {
                id: widget_id,
                label,
                value: binding,
                min: parse("min")?,
                max: parse("max")?,
                step: parse("step")?,
                disabled,
            }
        }
        Some(other) => {
            return Err(CookError::UnsupportedProperty {
                element: el.who.to_owned(),
                property: format!("input type={other}"),
            });
        }
        None => {
            return Err(CookError::MissingAttribute {
                element: el.who.to_owned(),
                attribute: "type",
            });
        }
    };
    node.style.text_color = el.color;
    Ok(())
}

/// A `select`: the bound value is chosen from `data-options`, and its label is the element text.
pub(super) fn select(el: &Element<'_>, node: &mut Node) -> Result<(), CookError> {
    let id = el.attr("id")?.clone();
    let selected = el.attr("data-bind")?.clone();
    let options = el.attr("data-options")?.clone();
    node.content = Content::Choice {
        id,
        label: Template::parse(text_of(el.handle).trim())?,
        options,
        selected,
        disabled: el.attrs.get("data-disabled").cloned(),
    };
    node.style.text_color = el.color;
    Ok(())
}
