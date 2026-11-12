//! Binding-driven layout, visibility and intrinsic text measurement updates.

use taffy::prelude as t;

use super::runtime::{ExpandedNode, Impact, NodeState, RowBinding};
use super::text::{layout_err, list_binding};
use crate::bindings::{Bindings, ListRecord, Value};
use crate::doc::{Align, Content, Direction, Display, Justify, Len, Node, Position};
use crate::error::UiError;
use crate::layout::TextMeasure;

pub(crate) struct UpdateContext<'a, 'b> {
    pub(crate) states: &'a mut [NodeState],
    pub(crate) changed: &'a [bool],
    pub(crate) tree: &'a mut t::TaffyTree<()>,
    pub(crate) bindings: &'a Bindings,
    pub(crate) measure: &'b mut dyn TextMeasure,
    pub(crate) scale: f32,
    pub(crate) short: f32,
    pub(crate) remeasure_all: bool,
}

impl UpdateContext<'_, '_> {
    pub(crate) fn apply(
        &mut self,
        node: &ExpandedNode,
        index: &mut usize,
    ) -> Result<bool, UiError> {
        let current = *index;
        *index += 1;
        let state = self
            .states
            .get_mut(current)
            .ok_or_else(|| UiError::Layout("node map is incomplete".to_owned()))?;
        if self.remeasure_all {
            state.fitted_width = None;
        }
        if let Content::Repeat { source, max, .. } = &node.content {
            let list = list_binding(self.bindings, source, &state.path)?;
            if list.records().len() > usize::from(*max) {
                return Err(UiError::RepeatTooLong {
                    name: source.clone(),
                    actual: list.records().len(),
                    max: *max,
                });
            }
        }
        let mut text_dirty = self.remeasure_all || !state.initialized;
        for (dependency_index, impact) in &state.dependencies {
            let was_changed = self
                .changed
                .get(*dependency_index)
                .copied()
                .unwrap_or(false);
            if was_changed {
                state.fitted_width = None;
                text_dirty |= text_content(node).is_some()
                    && (*impact == Impact::Layout || state.row.is_some());
            }
        }
        let row_visible = if let Some(row) = &state.row {
            let list = list_binding(self.bindings, &row.source, &state.path)?;
            usize::from(row.index.0) < list.records().len()
        } else {
            true
        };
        let visible = if row_visible {
            condition_value(node, self.bindings, &state.path, state.row.as_ref())?
        } else {
            false
        };
        let previous_natural = state.natural;
        if text_dirty
            && visible
            && let Some((text, size)) = text_content(node)
        {
            state.text.clear();
            if let Some(row) = &state.row {
                let record = row_record(self.bindings, row, &state.path)?;
                text.render_list_row(self.bindings, record, &state.path, &mut state.text)?;
            } else {
                text.render(self.bindings, &state.path, &mut state.text)?;
            }
            let px = size * self.scale;
            state.natural = Some((self.measure.width(px, &state.text), px));
        } else if !visible {
            state.text.clear();
            state.natural = None;
        }
        let natural_geometry_changed = match (previous_natural, state.natural) {
            (Some((old_width, old_height)), Some((new_width, new_height))) => {
                (matches!(node.layout.width, Len::Auto) && old_width != new_width)
                    || (matches!(node.layout.height, Len::Auto) && old_height != new_height)
            }
            (None, Some(_)) | (Some(_), None) => {
                matches!(node.layout.width, Len::Auto) || matches!(node.layout.height, Len::Auto)
            }
            (None, None) => false,
        };
        // Repeated rows reserve layout slots at load time. Dirtying them on a list resize makes
        // Taffy's flex pass allocate temporary item vectors on the UI update path.
        let node_dirty = self.remeasure_all
            || !state.initialized
            || (visible != state.visible && state.row.is_none())
            || natural_geometry_changed;
        if node_dirty {
            let mut style = style_of(node, self.scale, self.short, state.natural);
            if !visible && state.row.is_none() {
                style.display = t::Display::None;
            }
            let id = state.id;
            self.tree.set_style(id, style).map_err(layout_err)?;
            self.tree.mark_dirty(id).map_err(layout_err)?;
        }
        state.visible = visible;
        state.initialized = true;
        let mut geometry_dirty = node_dirty;
        for child in &node.children {
            geometry_dirty |= self.apply(child, index)?;
        }
        Ok(geometry_dirty)
    }
}

pub(crate) fn text_content(node: &Node) -> Option<(&crate::template::Template, f32)> {
    match &node.content {
        Content::Text { text, size, .. } => Some((text, *size)),
        Content::Action { label, .. }
        | Content::Toggle { label, .. }
        | Content::Slider { label, .. }
        | Content::Choice { label, .. } => Some((label, 20.0)),
        _ => None,
    }
}

fn row_record<'a>(
    bindings: &'a Bindings,
    row: &RowBinding,
    path: &str,
) -> Result<&'a ListRecord, UiError> {
    let list = list_binding(bindings, &row.source, path)?;
    list.records()
        .get(usize::from(row.index.0))
        .ok_or_else(|| UiError::UnboundVariable {
            node: path.to_owned(),
            name: format!("{}[{}]", row.source, row.index.0),
        })
}

pub(crate) fn binding_value<'a>(
    bindings: &'a Bindings,
    name: &str,
    row: Option<&RowBinding>,
    path: &str,
) -> Result<&'a Value, UiError> {
    let value = if let Some(field) = name.strip_prefix("item.") {
        let row = row.ok_or_else(|| UiError::UnboundVariable {
            node: path.to_owned(),
            name: name.to_owned(),
        })?;
        row_record(bindings, row, path)?.get(field)
    } else {
        bindings.get(name)
    };
    value.ok_or_else(|| UiError::UnboundVariable {
        node: path.to_owned(),
        name: name.to_owned(),
    })
}

pub(crate) fn condition_value(
    node: &Node,
    bindings: &Bindings,
    path: &str,
    row: Option<&RowBinding>,
) -> Result<bool, UiError> {
    let Some(condition) = &node.show_if else {
        return Ok(true);
    };
    let (name, want) = split_condition(condition);
    match binding_value(bindings, name, row, path)? {
        Value::Flag(value) => Ok(*value == want),
        _ => Err(UiError::BindingType {
            node: path.to_owned(),
            name: name.to_owned(),
            expected: "flag",
        }),
    }
}

pub(crate) fn split_condition(condition: &str) -> (&str, bool) {
    condition
        .strip_prefix('!')
        .map_or((condition, true), |name| (name, false))
}

pub(crate) fn length(value: Len, scale: f32, short: f32) -> t::Dimension {
    match value {
        Len::Auto => t::auto(),
        Len::Px(n) => t::length(n * scale),
        Len::Percent(n) => t::percent(n),
        Len::Short(n) => t::length(n * short),
    }
}
pub(crate) fn inset(value: Option<Len>, scale: f32, short: f32) -> t::LengthPercentageAuto {
    match value {
        None | Some(Len::Auto) => t::auto(),
        Some(Len::Px(n)) => t::length(n * scale),
        Some(Len::Percent(n)) => t::percent(n),
        Some(Len::Short(n)) => t::length(n * short),
    }
}
pub(crate) fn style_of(
    node: &Node,
    scale: f32,
    short: f32,
    natural: Option<(f32, f32)>,
) -> t::Style {
    let layout = &node.layout;
    let border = node.style.border.map_or(0.0, |border| border.width) * scale;
    let mut width = length(layout.width, scale, short);
    let mut height = length(layout.height, scale, short);
    if let Some((w, h)) = natural {
        if matches!(layout.width, Len::Auto) {
            width = t::length(w);
        }
        if matches!(layout.height, Len::Auto) {
            height = t::length(h);
        }
    }
    let (position, insets) = match layout.position {
        Position::Flow => (
            taffy::Position::Relative,
            taffy::Rect {
                left: t::auto(),
                right: t::auto(),
                top: t::auto(),
                bottom: t::auto(),
            },
        ),
        Position::Absolute {
            left,
            top,
            right,
            bottom,
        } => (
            taffy::Position::Absolute,
            taffy::Rect {
                left: inset(left, scale, short),
                right: inset(right, scale, short),
                top: inset(top, scale, short),
                bottom: inset(bottom, scale, short),
            },
        ),
    };
    t::Style {
        display: match layout.display {
            Display::Block => t::Display::Block,
            Display::Flex => t::Display::Flex,
        },
        position,
        inset: insets,
        size: taffy::Size { width, height },
        margin: taffy::Rect {
            left: t::length(layout.margin * scale),
            right: t::length(layout.margin * scale),
            top: t::length(layout.margin * scale),
            bottom: t::length(layout.margin * scale),
        },
        padding: taffy::Rect {
            left: t::length(layout.padding * scale),
            right: t::length(layout.padding * scale),
            top: t::length(layout.padding * scale),
            bottom: t::length(layout.padding * scale),
        },
        border: taffy::Rect {
            left: t::length(border),
            right: t::length(border),
            top: t::length(border),
            bottom: t::length(border),
        },
        gap: taffy::Size {
            width: t::length(layout.gap * scale),
            height: t::length(layout.gap * scale),
        },
        flex_direction: match layout.direction {
            Direction::Row => t::FlexDirection::Row,
            Direction::Column => t::FlexDirection::Column,
        },
        align_items: match layout.align {
            Align::Start => t::AlignItems::FLEX_START,
            Align::Center => t::AlignItems::CENTER,
            Align::End => t::AlignItems::FLEX_END,
            Align::Stretch => t::AlignItems::STRETCH,
        },
        justify_content: match layout.justify {
            Justify::Start => t::JustifyContent::FLEX_START,
            Justify::Center => t::JustifyContent::CENTER,
            Justify::End => t::JustifyContent::FLEX_END,
            Justify::SpaceBetween => t::JustifyContent::SPACE_BETWEEN,
            Justify::SpaceAround => t::JustifyContent::SPACE_AROUND,
        },
        flex_grow: layout.grow,
        flex_shrink: 0.0,
        ..t::Style::default()
    }
}
