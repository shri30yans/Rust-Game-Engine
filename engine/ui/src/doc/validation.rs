//! Cook and load boundary validation for document trees.

use std::collections::BTreeSet;

use super::content::{Content, Node, TextFit};
use super::layout::{Len, Position};
use crate::brush::{Background, check_gradient};
use crate::error::UiError;
use crate::template::Template;

#[derive(Default)]
pub(super) struct Seen {
    pub(super) ids: BTreeSet<String>,
    pub(super) buttons: BTreeSet<u8>,
    pub(super) widgets: BTreeSet<String>,
    pub(super) actions: BTreeSet<String>,
}

fn validate_background(background: &Background, path: &str) -> Result<(), UiError> {
    if let Background::LinearGradient { angle_deg, stops } = background {
        check_gradient(*angle_deg, stops).map_err(|reason| invalid(path, reason))?;
    }
    Ok(())
}

pub(super) fn invalid(node: &str, reason: &'static str) -> UiError {
    UiError::DocInvalid {
        node: node.to_owned(),
        reason,
    }
}

fn finite_at_least(value: f32, min: f32) -> bool {
    value.is_finite() && value >= min
}

fn len_ok(len: Len) -> bool {
    match len {
        Len::Auto => true,
        Len::Px(v) => finite_at_least(v, 0.0),
        Len::Percent(v) | Len::Short(v) => v.is_finite() && (0.0..=1.0).contains(&v),
    }
}

pub(super) fn validate(node: &Node, path: &str, seen: &mut Seen) -> Result<(), UiError> {
    if let TextFit::Shrink { minimum } = node.fit
        && !(minimum.is_finite() && (0.0..=1.0).contains(&minimum) && minimum > 0.0)
    {
        return Err(invalid(
            path,
            "text fit minimum must be finite and in (0, 1]",
        ));
    }
    if let Some(id) = &node.id
        && !seen.ids.insert(id.clone())
    {
        return Err(invalid(path, "duplicate node id"));
    }
    let l = &node.layout;
    if !(len_ok(l.width) && len_ok(l.height)) {
        return Err(invalid(
            path,
            "width and height must be finite and non-negative; fractions 0..=1",
        ));
    }
    if let Position::Absolute {
        left,
        top,
        right,
        bottom,
    } = l.position
    {
        for edge in [left, top, right, bottom].into_iter().flatten() {
            if matches!(edge, Len::Auto) || !len_ok(edge) {
                return Err(invalid(
                    path,
                    "an absolute edge needs a finite px, percent or short value",
                ));
            }
        }
    }
    if !(finite_at_least(l.gap, 0.0)
        && finite_at_least(l.padding, 0.0)
        && finite_at_least(l.margin, 0.0)
        && finite_at_least(l.grow, 0.0))
    {
        return Err(invalid(
            path,
            "gap, padding, margin and grow must be finite and non-negative",
        ));
    }
    validate_background(&node.style.background, path)?;
    let s = &node.style;
    if !finite_at_least(s.radius, 0.0)
        || !(s.opacity.is_finite() && (0.0..=1.0).contains(&s.opacity))
        || !(s.skew.is_finite() && s.skew.abs() <= 45.0)
    {
        return Err(invalid(
            path,
            "radius must be non-negative, opacity within 0..=1 and skew within -45..=45 degrees",
        ));
    }
    if let Some(border) = s.border
        && !finite_at_least(border.width, 0.0)
    {
        return Err(invalid(
            path,
            "border width must be finite and non-negative",
        ));
    }
    for delta in [
        &s.states.hover,
        &s.states.focus,
        &s.states.pressed,
        &s.states.disabled,
    ]
    .into_iter()
    .flatten()
    {
        if let Some(background) = &delta.background {
            validate_background(background, path)?;
        }
        if delta
            .opacity
            .is_some_and(|value| !(value.is_finite() && (0.0..=1.0).contains(&value)))
            || delta
                .scale
                .is_some_and(|value| !(value.is_finite() && value > 0.0 && value <= 4.0))
            || delta
                .border
                .is_some_and(|border| !finite_at_least(border.width, 0.0))
        {
            return Err(invalid(
                path,
                "state opacity, scale or border width is out of range",
            ));
        }
    }
    if let Some(flag) = &node.show_if {
        let name = flag.strip_prefix('!').unwrap_or(flag);
        if name.is_empty() {
            return Err(invalid(path, "show_if names no variable"));
        }
    }
    match &node.content {
        Content::None => {}
        Content::Text { size, .. } => {
            if !(size.is_finite() && *size > 0.0) {
                return Err(invalid(path, "text size must be finite and above zero"));
            }
            if !node.children.is_empty() {
                return Err(invalid(path, "a text node has no children"));
            }
        }
        Content::Bar { value, .. } => {
            if value.is_empty() {
                return Err(invalid(path, "a bar names no variable"));
            }
            if !node.children.is_empty() {
                return Err(invalid(path, "a bar has no children"));
            }
        }
        Content::Button { id, held, .. } => {
            if held.is_empty() {
                return Err(invalid(path, "a button names no held variable"));
            }
            if !matches!(node.layout.position, Position::Absolute { .. }) {
                return Err(invalid(
                    path,
                    "a button is absolutely positioned, so its touch area never depends on text",
                ));
            }
            if !seen.buttons.insert(*id) {
                return Err(invalid(path, "duplicate button id"));
            }
            if !node.children.is_empty() {
                return Err(invalid(path, "a button has no children"));
            }
        }
        Content::Action {
            id,
            label,
            disabled,
        }
        | Content::Toggle {
            id,
            label,
            disabled,
            ..
        }
        | Content::Choice {
            id,
            label,
            disabled,
            ..
        }
        | Content::Slider {
            id,
            label,
            disabled,
            ..
        } => {
            validate_widget(id, label, disabled.as_deref(), node, path, seen)?;
            if let Content::Slider { min, max, step, .. } = &node.content
                && !(min.is_finite()
                    && max.is_finite()
                    && step.is_finite()
                    && min < max
                    && *step > 0.0)
            {
                return Err(invalid(
                    path,
                    "slider range must be finite, increasing and have a positive step",
                ));
            }
        }
        Content::Tabs {
            id,
            tabs,
            selected,
            disabled,
        } => {
            validate_widget(
                id,
                &Template::parse("")?,
                disabled.as_deref(),
                node,
                path,
                seen,
            )?;
            if tabs.is_empty() || selected.is_empty() {
                return Err(invalid(path, "tabs must name list and selected bindings"));
            }
        }
        Content::Repeat {
            source,
            max,
            template,
        } => {
            if source.is_empty() || *max == 0 || *max > 1024 {
                return Err(invalid(
                    path,
                    "repeat source and max must be valid (1..=1024)",
                ));
            }
            validate(template, &format!("{path}/template"), seen)?;
            if !node.children.is_empty() {
                return Err(invalid(
                    path,
                    "repeat owns its template and has no children",
                ));
            }
        }
    }
    if !matches!(
        node.content,
        Content::Action { .. }
            | Content::Toggle { .. }
            | Content::Slider { .. }
            | Content::Choice { .. }
            | Content::Tabs { .. }
    ) && (s.states.hover.is_some()
        || s.states.focus.is_some()
        || s.states.pressed.is_some()
        || s.states.disabled.is_some())
    {
        return Err(invalid(
            path,
            "state styles are allowed only on focusable widgets",
        ));
    }
    for (index, child) in node.children.iter().enumerate() {
        validate(child, &format!("{path}/{index}"), seen)?;
    }
    Ok(())
}

fn validate_widget(
    id: &str,
    label: &Template,
    disabled: Option<&str>,
    node: &Node,
    path: &str,
    seen: &mut Seen,
) -> Result<(), UiError> {
    if id.is_empty() || !seen.widgets.insert(id.to_owned()) {
        return Err(invalid(path, "widget id is empty or duplicated"));
    }
    if node.id.as_deref() != Some(id) && !seen.ids.insert(id.to_owned()) {
        return Err(invalid(path, "widget id duplicates a document node id"));
    }
    if matches!(node.content, Content::Action { .. }) {
        seen.actions.insert(id.to_owned());
    }
    if disabled.is_some_and(str::is_empty) {
        return Err(invalid(path, "disabled binding names no variable"));
    }
    if label.variables().any(str::is_empty) {
        return Err(invalid(path, "widget label contains an invalid binding"));
    }
    if !node.children.is_empty() {
        return Err(invalid(path, "a widget has no children"));
    }
    Ok(())
}
