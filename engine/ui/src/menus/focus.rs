//! Focus order, restoration and spatial navigation for menu widgets.

use engine_core::Rect;
use glam::Vec2;

use crate::doc::Content;
use crate::error::UiError;
use crate::focus::{FocusRect, NavInput, RowIndex, WidgetId};
use crate::layout::WidgetRegion;

use super::runtime::{OpenScreen, WidgetInstance};

pub(super) fn gather_widget_handles(
    node: &crate::doc::Node,
    row: Option<(String, RowIndex)>,
    out: &mut Vec<WidgetInstance>,
    base_handles: &mut Vec<(String, WidgetId)>,
    next: &mut u32,
) -> Result<(), UiError> {
    let name = match &node.content {
        Content::Action { id, .. }
        | Content::Toggle { id, .. }
        | Content::Slider { id, .. }
        | Content::Choice { id, .. }
        | Content::Tabs { id, .. } => Some(id.as_str()),
        _ => None,
    };
    if let Some(name) = name {
        let base_id = if let Some((_, id)) = base_handles.iter().find(|(known, _)| known == name) {
            *id
        } else {
            let id = allocate_widget_id(next)?;
            base_handles.push((name.to_owned(), id));
            id
        };
        let focus_id = match row {
            Some((_, RowIndex(0))) | None => base_id,
            Some(_) => allocate_widget_id(next)?,
        };
        out.push(WidgetInstance {
            name: name.to_owned(),
            base_id,
            focus_id,
            row: row.as_ref().map(|(_, index)| *index),
            row_source: row.as_ref().map(|(source, _)| source.clone()),
        });
    }
    if let Content::Repeat {
        source,
        max,
        template,
    } = &node.content
    {
        for index in 0..usize::from(*max) {
            let index = u16::try_from(index).map_err(|_| UiError::IdExhausted)?;
            gather_widget_handles(
                template,
                Some((source.clone(), RowIndex(index))),
                out,
                base_handles,
                next,
            )?;
        }
        return Ok(());
    }
    for child in &node.children {
        gather_widget_handles(child, row.clone(), out, base_handles, next)?;
    }
    Ok(())
}

pub(super) fn allocate_widget_id(next: &mut u32) -> Result<WidgetId, UiError> {
    let id = WidgetId(*next);
    *next = next.checked_add(1).ok_or(UiError::IdExhausted)?;
    Ok(id)
}
pub(super) fn fill_focus_rects(
    regions: &[WidgetRegion],
    handles: &[WidgetInstance],
    out: &mut Vec<FocusRect>,
) {
    out.clear();
    for region in regions.iter().filter(|region| !region.disabled) {
        if let Some(handle) = handles.get(region.widget_index) {
            let r = region.rect;
            out.push(FocusRect {
                id: handle.focus_id,
                min: Vec2::new(r.x, r.y),
                max: Vec2::new(r.x + r.w, r.y + r.h),
            });
        }
    }
}
pub(super) fn restored_visible_focus(
    screen: &OpenScreen,
    old_focus: Option<WidgetId>,
) -> Option<WidgetId> {
    if let Some(old_focus) = old_focus
        && let Some(previous) = screen
            .widget_handles
            .iter()
            .find(|widget| widget.focus_id == old_focus)
        && let Some(previous_row) = previous.row
        && let Some(nearest) = screen
            .widget_handles
            .iter()
            .filter(|widget| {
                widget.name == previous.name
                    && widget.row.is_some()
                    && screen
                        .focus_rects
                        .iter()
                        .any(|rect| rect.id == widget.focus_id)
            })
            .min_by_key(|widget| {
                widget
                    .row
                    .map_or(u16::MAX, |row| row.0.abs_diff(previous_row.0))
            })
    {
        return Some(nearest.focus_id);
    }
    let default = screen
        .layout
        .document()
        .screen()
        .and_then(|definition| definition.default_focus.as_deref());
    default
        .and_then(|name| {
            screen
                .widget_handles
                .iter()
                .find(|widget| {
                    widget.name == name
                        && screen
                            .focus_rects
                            .iter()
                            .any(|rect| rect.id == widget.focus_id)
                })
                .map(|widget| widget.focus_id)
        })
        .or_else(|| screen.focus_rects.first().map(|rect| rect.id))
}
pub(super) fn wrap_focus(
    current: WidgetId,
    direction: NavInput,
    rects: &[FocusRect],
) -> Option<WidgetId> {
    let pos = rects.iter().position(|rect| rect.id == current)?;
    match direction {
        NavInput::Next | NavInput::Right | NavInput::Down => {
            rects.get((pos + 1) % rects.len()).map(|rect| rect.id)
        }
        NavInput::Prev | NavInput::Left | NavInput::Up => rects
            .get(pos.checked_sub(1).unwrap_or(rects.len().saturating_sub(1)))
            .map(|rect| rect.id),
    }
}
fn contains(rect: Rect, point: Vec2) -> bool {
    point.x >= rect.x
        && point.y >= rect.y
        && point.x <= rect.x + rect.w
        && point.y <= rect.y + rect.h
}
pub(super) fn contains_widget(region: &WidgetRegion, point: Vec2) -> bool {
    let local = region.skew.map_or(point, |skew| {
        Vec2::new(point.x - skew.lean * (point.y - skew.pivot_y), point.y)
    });
    contains(region.rect, local)
}
