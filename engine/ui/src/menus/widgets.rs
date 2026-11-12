//! Menu action, toggle, slider and choice binding behavior.

use crate::bindings::{Bindings, Value};
use crate::doc::Content;
use crate::error::UiError;
use crate::focus::{NavInput, WidgetId};
use crate::layout::WidgetRegion;

use super::input::{MenuInput, UiEvent};

use super::runtime::{OpenScreen, WidgetInstance};

fn find_widget<'a>(node: &'a crate::doc::Node, id: &str) -> Option<&'a Content> {
    let found = match &node.content {
        Content::Action { id: name, .. }
        | Content::Toggle { id: name, .. }
        | Content::Slider { id: name, .. }
        | Content::Choice { id: name, .. }
        | Content::Tabs { id: name, .. }
            if name == id =>
        {
            Some(&node.content)
        }
        _ => None,
    };
    found
        .or_else(|| match &node.content {
            Content::Repeat { template, .. } => find_widget(template, id),
            _ => None,
        })
        .or_else(|| {
            node.children
                .iter()
                .find_map(|child| find_widget(child, id))
        })
}
pub(super) fn activate_widget(
    screen: &OpenScreen,
    region: &WidgetRegion,
    input: &MenuInput,
    bindings: &Bindings,
    events: &mut Vec<UiEvent>,
) -> Result<(), UiError> {
    let Some(widget) = screen.widget_handles.get(region.widget_index) else {
        return Ok(());
    };
    let Some(content) = find_widget(screen.layout.document().root(), &widget.name) else {
        return Ok(());
    };
    match content {
        Content::Action { .. } => events.push(UiEvent::Action(widget.base_id, widget.row)),
        Content::Toggle { value, .. } => {
            if let Ok(Value::Flag(current)) = widget_value(bindings, widget, value) {
                events.push(UiEvent::Toggled(widget.base_id, widget.row, !current));
            }
        }
        Content::Slider {
            value,
            min,
            max,
            step,
            ..
        } => {
            if let Ok(Value::Num(current)) = widget_value(bindings, widget, value) {
                let direction = if input.nav == Some(NavInput::Left) {
                    -1.0
                } else {
                    1.0
                };
                events.push(UiEvent::Changed(
                    widget.base_id,
                    widget.row,
                    (*current + direction * step).clamp(*min, *max),
                ));
            }
        }
        Content::Choice {
            options, selected, ..
        } => emit_selection(widget, options, selected, true, bindings, events)?,
        Content::Tabs { tabs, selected, .. } => {
            emit_selection(widget, tabs, selected, input.tab_next, bindings, events)?
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn is_tabs_widget(screen: &OpenScreen, focus: WidgetId) -> bool {
    let Some(widget) = screen
        .widget_handles
        .iter()
        .find(|widget| widget.focus_id == focus)
    else {
        return false;
    };
    matches!(
        find_widget(screen.layout.document().root(), &widget.name),
        Some(Content::Tabs { .. })
    )
}

pub(super) fn adjust_selected_widget(
    screen: &OpenScreen,
    focus: WidgetId,
    forward: bool,
    bindings: &Bindings,
    events: &mut Vec<UiEvent>,
) -> Result<bool, UiError> {
    let Some((widget_index, widget)) = screen
        .widget_handles
        .iter()
        .enumerate()
        .find(|(_, widget)| widget.focus_id == focus)
    else {
        return Ok(false);
    };
    let Some(region) = screen.layout.draw_list_widgets().get(widget_index) else {
        return Ok(false);
    };
    if region.disabled {
        return Ok(false);
    }
    let Some(content) = find_widget(screen.layout.document().root(), &widget.name) else {
        return Ok(false);
    };
    match content {
        Content::Slider {
            value,
            min,
            max,
            step,
            ..
        } => {
            let Ok(Value::Num(current)) = widget_value(bindings, widget, value) else {
                return Ok(false);
            };
            let direction = if forward { 1.0 } else { -1.0 };
            events.push(UiEvent::Changed(
                widget.base_id,
                widget.row,
                (*current + direction * step).clamp(*min, *max),
            ));
            Ok(true)
        }
        Content::Choice {
            options, selected, ..
        } => {
            emit_selection(widget, options, selected, forward, bindings, events)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(super) fn adjust_widget(
    screen: &OpenScreen,
    region: &WidgetRegion,
    forward: bool,
    bindings: &Bindings,
    events: &mut Vec<UiEvent>,
) -> Result<(), UiError> {
    let Some(widget) = screen.widget_handles.get(region.widget_index) else {
        return Ok(());
    };
    if let Some(Content::Tabs { tabs, selected, .. }) =
        find_widget(screen.layout.document().root(), &widget.name)
    {
        emit_selection(widget, tabs, selected, forward, bindings, events)?;
    }
    Ok(())
}

pub(super) fn emit_selection(
    widget: &WidgetInstance,
    list_name: &str,
    selected_name: &str,
    forward: bool,
    bindings: &Bindings,
    events: &mut Vec<UiEvent>,
) -> Result<(), UiError> {
    let Value::List(list) = widget_value(bindings, widget, list_name)? else {
        return Err(UiError::BindingType {
            node: "menu".to_owned(),
            name: list_name.to_owned(),
            expected: "list",
        });
    };
    let count = u16::try_from(list.records().len())
        .map_err(|_| UiError::ChoiceTooLarge(list_name.to_owned()))?;
    if count == 0 {
        return Ok(());
    }
    let Value::Num(current) = widget_value(bindings, widget, selected_name)? else {
        return Err(UiError::BindingType {
            node: "menu".to_owned(),
            name: selected_name.to_owned(),
            expected: "number",
        });
    };
    let Some(index) = index_from_f32(*current) else {
        return Ok(());
    };
    let next = if forward {
        index
            .checked_add(1)
            .filter(|next| *next < count)
            .unwrap_or(0)
    } else {
        index.checked_sub(1).unwrap_or(count - 1)
    };
    events.push(UiEvent::Selected(widget.base_id, widget.row, next));
    Ok(())
}

pub(super) fn widget_value<'a>(
    bindings: &'a Bindings,
    widget: &WidgetInstance,
    name: &str,
) -> Result<&'a Value, UiError> {
    let value = if let Some(field) = name.strip_prefix("item.") {
        let source = widget
            .row_source
            .as_deref()
            .ok_or_else(|| UiError::UnboundVariable {
                node: "menu".to_owned(),
                name: name.to_owned(),
            })?;
        let row = widget.row.ok_or_else(|| UiError::UnboundVariable {
            node: "menu".to_owned(),
            name: name.to_owned(),
        })?;
        let Some(Value::List(list)) = bindings.get(source) else {
            return Err(UiError::UnboundVariable {
                node: "menu".to_owned(),
                name: source.to_owned(),
            });
        };
        list.records()
            .get(usize::from(row.0))
            .and_then(|record| record.get(field))
    } else {
        bindings.get(name)
    };
    value.ok_or_else(|| UiError::UnboundVariable {
        node: "menu".to_owned(),
        name: name.to_owned(),
    })
}

fn index_from_f32(value: f32) -> Option<u16> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return None;
    }
    (0..=u16::MAX).find(|index| f32::from(*index) == value)
}
