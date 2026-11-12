//! Text fitting and primitive helpers shared by layout painting.

use super::runtime::{ExpandedNode, NodeState, RowBinding};
use crate::brush::GradientStop;
use crate::color::{Color, Srgb8};
use crate::doc::TextAlign;
use engine_core::Rect;

use super::update::binding_value;
use crate::bindings::{Bindings, Value};
use crate::brush::Background;
use crate::doc::{TextFit, TextOverflow};
use crate::error::UiError;
use crate::layout::{DrawList, Paint, Primitive, TextMeasure, TextRunBounds};

pub(crate) struct TextRunSpec {
    pub(crate) start: usize,
    pub(crate) rect: Rect,
    pub(crate) parent: Rect,
    pub(crate) base_px: f32,
    pub(crate) color: Paint,
    pub(crate) align: TextAlign,
    pub(crate) node_index: usize,
    pub(crate) fit: TextFit,
    pub(crate) overflow: TextOverflow,
}

pub(crate) fn fit_text(
    measure: &mut dyn TextMeasure,
    ellipsis_scratch: &mut String,
    out: &mut DrawList,
    state: &mut NodeState,
    spec: TextRunSpec,
) -> Result<(), UiError> {
    let TextRunSpec {
        start,
        rect,
        parent,
        base_px,
        color,
        align,
        node_index,
        fit,
        overflow: _overflow,
    } = spec;
    let original_end = out.text.len();
    let original = out
        .text
        .get(start..original_end)
        .ok_or_else(|| UiError::Layout("text range is invalid".to_owned()))?;
    let mut px = base_px;
    let mut width = state
        .fitted_width
        .filter(|(measured_px, _)| *measured_px == px)
        .map_or_else(|| measure.width(px, original), |(_, width)| width);
    state.fitted_width = Some((px, width));
    if let TextFit::Shrink { minimum } = fit
        && width > rect.w
        && width > 0.0
    {
        px *= (rect.w / width).max(minimum).min(1.0);
        width = measure.width(px, original);
    }
    if width > rect.w {
        ellipsis_scratch.clear();
        for character in original.chars() {
            ellipsis_scratch.push(character);
            ellipsis_scratch.push('…');
            let candidate_width = measure.width(px, ellipsis_scratch);
            ellipsis_scratch.pop();
            if candidate_width > rect.w {
                ellipsis_scratch.pop();
                break;
            }
        }
        let ellipsis_width = measure.width(px, "…");
        if ellipsis_width <= rect.w {
            ellipsis_scratch.push('…');
        } else {
            ellipsis_scratch.clear();
        }
        out.text.truncate(start);
        out.text.push_str(ellipsis_scratch);
        width = measure.width(px, ellipsis_scratch);
    }
    let end = out.text.len();
    out.prims.push(Primitive::Text {
        rect,
        px,
        color,
        align,
        range: start..end,
    });
    if end > start {
        out.text_runs.push(TextRunBounds {
            node: node_index,
            rect,
            parent,
            width,
            px,
        });
    }
    Ok(())
}

pub(crate) fn list_binding<'a>(
    bindings: &'a Bindings,
    name: &str,
    path: &str,
) -> Result<&'a crate::bindings::ListBinding, UiError> {
    match bindings.get(name) {
        Some(Value::List(list)) => Ok(list),
        Some(_) => Err(UiError::BindingType {
            node: path.to_owned(),
            name: name.to_owned(),
            expected: "list",
        }),
        None => Err(UiError::UnboundVariable {
            node: path.to_owned(),
            name: name.to_owned(),
        }),
    }
}

pub(crate) fn list_binding_in<'a>(
    bindings: &'a Bindings,
    name: &str,
    path: &str,
    row: Option<&RowBinding>,
) -> Result<&'a crate::bindings::ListBinding, UiError> {
    match binding_value(bindings, name, row, path)? {
        Value::List(list) => Ok(list),
        _ => Err(UiError::BindingType {
            node: path.to_owned(),
            name: name.to_owned(),
            expected: "list",
        }),
    }
}

pub(crate) fn selected_label<'a>(
    bindings: &'a Bindings,
    list_name: &str,
    selected_name: &str,
    path: &str,
    row: Option<&RowBinding>,
) -> Result<&'a str, UiError> {
    let list = list_binding_in(bindings, list_name, path, row)?;
    let selected = number_binding(bindings, selected_name, path, row)?;
    let Some(index) = index_from_f32(selected) else {
        return Err(UiError::DocInvalid {
            node: path.to_owned(),
            reason: "selection must be an integer in the list",
        });
    };
    let Some(record) = list.records().get(usize::from(index)) else {
        return Err(UiError::DocInvalid {
            node: path.to_owned(),
            reason: "selection is outside the list",
        });
    };
    for key in ["label", "name"] {
        if let Some(Value::Text(value)) = record.get(key) {
            return Ok(value);
        }
    }
    Err(UiError::BindingType {
        node: path.to_owned(),
        name: list_name.to_owned(),
        expected: "list records with a text label or name field",
    })
}

pub(crate) fn index_from_f32(value: f32) -> Option<u16> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return None;
    }
    (0..=u16::MAX).find(|index| f32::from(*index) == value)
}

pub(crate) fn skip_node(node: &ExpandedNode, index: &mut usize) {
    *index += 1;
    for child in &node.children {
        skip_node(child, index);
    }
}
pub(crate) fn number_binding(
    bindings: &Bindings,
    name: &str,
    path: &str,
    row: Option<&RowBinding>,
) -> Result<f32, UiError> {
    match binding_value(bindings, name, row, path)? {
        Value::Num(value) => Ok(*value),
        _ => Err(UiError::BindingType {
            node: path.to_owned(),
            name: name.to_owned(),
            expected: "number",
        }),
    }
}
pub(crate) fn flag_binding(
    bindings: &Bindings,
    name: &str,
    path: &str,
    row: Option<&RowBinding>,
) -> Result<bool, UiError> {
    match binding_value(bindings, name, row, path)? {
        Value::Flag(value) => Ok(*value),
        _ => Err(UiError::BindingType {
            node: path.to_owned(),
            name: name.to_owned(),
            expected: "flag",
        }),
    }
}
pub(crate) fn layout_err(error: impl std::fmt::Display) -> UiError {
    UiError::Layout(error.to_string())
}

pub(crate) fn push_background(
    out: &mut DrawList,
    rect: Rect,
    radius: f32,
    opacity: f32,
    background: &Background,
) {
    match background {
        Background::None => {}
        Background::Solid(color) => {
            out.prims.push(Primitive::Fill {
                rect,
                radius,
                color: Paint::of(*color, opacity),
            });
        }
        Background::LinearGradient { angle_deg, stops } => {
            // The document validator guarantees two to four stops in order (check_gradient).
            let mut packed = [GradientStop {
                pos: 0.0,
                color: Color {
                    rgb: Srgb8 { r: 0, g: 0, b: 0 },
                    a: 0,
                },
            }; 4];
            for (slot, stop) in packed.iter_mut().zip(stops.iter()) {
                *slot = *stop;
            }
            out.prims.push(Primitive::LinearGradient {
                rect,
                radius,
                angle_deg: *angle_deg,
                stops: packed,
                stop_count: u8::try_from(stops.len().min(4)).unwrap_or(4),
            });
        }
    }
}
