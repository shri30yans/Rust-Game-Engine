//! Painter-order primitive generation from the retained Taffy tree.

use engine_core::{Rect, TouchRegion, TouchRegionId, TouchShape, Viewport};
use taffy::prelude as t;

use super::repeat::widget_id;
use super::runtime::{ExpandedNode, NodeState};
use super::text::{
    TextRunSpec, fit_text, flag_binding, index_from_f32, layout_err, list_binding_in,
    number_binding, push_background, selected_label, skip_node,
};
use crate::bindings::Bindings;
use crate::brush::Background;
use crate::doc::{Content, TextAlign};
use crate::error::UiError;
use crate::focus::RowIndex;
use crate::layout::{
    DrawList, Paint, Primitive, Skew, TextMeasure, TouchRegionBounds, WidgetRegion,
    WidgetVisualState,
};

pub(crate) struct DrawContext<'a> {
    pub(crate) states: &'a mut [NodeState],
    pub(crate) widget_states: &'a [(String, Option<RowIndex>, WidgetVisualState)],
    pub(crate) tree: &'a t::TaffyTree<()>,
    pub(crate) viewport: Viewport,
    pub(crate) scale: f32,
    pub(crate) bindings: &'a Bindings,
    pub(crate) measure: &'a mut dyn TextMeasure,
    pub(crate) ellipsis_scratch: &'a mut String,
    pub(crate) out: &'a mut DrawList,
}

impl DrawContext<'_> {
    pub(crate) fn apply(
        &mut self,
        node: &ExpandedNode,
        index: &mut usize,
        origin: (f32, f32),
        parent_rect: Rect,
        parent_opacity: f32,
        outer_skew: Option<Skew>,
    ) -> Result<(), UiError> {
        let current = *index;
        *index += 1;
        let state = self
            .states
            .get_mut(current)
            .ok_or_else(|| UiError::Layout("node map is incomplete".to_owned()))?;
        if !state.visible {
            for child in &node.children {
                skip_node(child, index);
            }
            return Ok(());
        }
        let id = state.id;
        let layout = self.tree.layout(id).map_err(layout_err)?;
        let rect = Rect {
            x: origin.0 + layout.location.x,
            y: origin.1 + layout.location.y,
            w: layout.size.width,
            h: layout.size.height,
        };
        let visual_state = widget_id(&node.content)
            .and_then(|id| {
                self.widget_states
                    .iter()
                    .find(|(name, row, _)| {
                        name == id && *row == state.row.as_ref().map(|row| row.index)
                    })
                    .map(|(_, _, state)| *state)
            })
            .unwrap_or_default();
        let state_delta = if visual_state.disabled {
            node.style.states.disabled.clone()
        } else if visual_state.pressed {
            node.style.states.pressed.clone()
        } else if visual_state.focused {
            node.style.states.focus.clone()
        } else if visual_state.hovered {
            node.style.states.hover.clone()
        } else {
            None
        };
        let delta = state_delta.as_ref();
        let opacity = parent_opacity
            * delta
                .and_then(|state| state.opacity)
                .unwrap_or(node.style.opacity);
        let background = delta
            .and_then(|state| state.background.clone())
            .or_else(|| {
                delta
                    .and_then(|state| state.fill)
                    .map(Background::Solid)
                    .or_else(|| {
                        if !matches!(node.style.background, Background::None) {
                            Some(node.style.background.clone())
                        } else {
                            node.style.fill.map(Background::Solid)
                        }
                    })
            })
            .unwrap_or(Background::None);
        let border = delta.and_then(|state| state.border).or(node.style.border);
        let text_color = delta
            .and_then(|state| state.text_color)
            .or(node.style.text_color);
        let paint_rect = delta
            .and_then(|state| state.scale)
            .map_or(rect, |scale| Rect {
                x: rect.x - rect.w * (scale - 1.0) * 0.5,
                y: rect.y - rect.h * (scale - 1.0) * 0.5,
                w: rect.w * scale,
                h: rect.h * scale,
            });
        let radius = node.style.radius * self.scale;
        let skew = if node.style.skew == 0.0 {
            outer_skew
        } else {
            let own = Some(Skew {
                lean: -node.style.skew.to_radians().tan(),
                pivot_y: rect.y + rect.h * 0.5,
            });
            self.out.prims.push(Primitive::Skew(own));
            own
        };
        push_background(self.out, paint_rect, radius, opacity, &background);
        if let Some(border) = border {
            self.out.prims.push(Primitive::Outline {
                rect: paint_rect,
                radius,
                width: border.width * self.scale,
                color: Paint::of(border.color, opacity),
            });
        }
        match &node.content {
            Content::None => {}
            Content::Text {
                size, color, align, ..
            } => {
                let start = self.out.text.len();
                self.out.text.push_str(&state.text);
                fit_text(
                    self.measure,
                    self.ellipsis_scratch,
                    self.out,
                    state,
                    TextRunSpec {
                        start,
                        rect: paint_rect,
                        parent: parent_rect,
                        base_px: size * self.scale,
                        color: Paint::of(text_color.unwrap_or(*color), opacity),
                        align: *align,
                        node_index: current,
                        fit: node.fit,
                        overflow: node.text_overflow,
                    },
                )?;
            }
            Content::Bar { value, color } => {
                let fraction =
                    number_binding(self.bindings, value, &state.path, state.row.as_ref())?
                        .clamp(0.0, 1.0);
                self.out.prims.push(Primitive::Fill {
                    rect: Rect {
                        w: rect.w * fraction,
                        ..rect
                    },
                    radius,
                    color: Paint::of(*color, opacity),
                });
            }
            Content::Button {
                id,
                color,
                held,
                drag_look,
                ..
            } => {
                let diameter = rect.w.min(rect.h);
                let (cx, cy, r) = (rect.x + rect.w * 0.5, rect.y + rect.h * 0.5, diameter * 0.5);
                let is_held = flag_binding(self.bindings, held, &state.path, state.row.as_ref())?;
                self.out.prims.push(Primitive::Disc {
                    cx,
                    cy,
                    radius: r,
                    color: Paint {
                        rgb: color.rgb,
                        alpha: if is_held { 0.45 } else { 0.18 } * opacity,
                    },
                });
                self.out.prims.push(Primitive::Ring {
                    cx,
                    cy,
                    radius: r,
                    width: (r * 0.08).max(1.0),
                    color: Paint::of(*color, opacity),
                });
                let start = self.out.text.len();
                self.out.text.push_str(&state.text);
                fit_text(
                    self.measure,
                    self.ellipsis_scratch,
                    self.out,
                    state,
                    TextRunSpec {
                        start,
                        rect: Rect {
                            x: cx - r,
                            y: cy - r * 0.35,
                            w: r * 2.0,
                            h: r * 0.7,
                        },
                        parent: rect,
                        base_px: r * 0.7,
                        color: Paint::of(*color, opacity),
                        align: TextAlign::Center,
                        node_index: current,
                        fit: node.fit,
                        overflow: node.text_overflow,
                    },
                )?;
                let touch_rect = Rect {
                    x: cx - r,
                    y: cy - r,
                    w: r * 2.0,
                    h: r * 2.0,
                };
                self.out.touch_bounds.push(TouchRegionBounds {
                    id: TouchRegionId(*id),
                    rect: touch_rect,
                });
                if let Some(rect) = self.viewport.clip(touch_rect) {
                    self.out.touch_regions.push(TouchRegion {
                        id: TouchRegionId(*id),
                        rect,
                        shape: TouchShape::Circle,
                        drag_look: *drag_look,
                    });
                }
            }
            Content::Action { id, disabled, .. }
            | Content::Toggle { id, disabled, .. }
            | Content::Slider { id, disabled, .. }
            | Content::Choice { id, disabled, .. } => {
                match &node.content {
                    Content::Toggle { value, .. } => {
                        let _ =
                            flag_binding(self.bindings, value, &state.path, state.row.as_ref())?;
                    }
                    Content::Slider {
                        value, min, max, ..
                    } => {
                        let current =
                            number_binding(self.bindings, value, &state.path, state.row.as_ref())?;
                        if !current.is_finite() || current < *min || current > *max {
                            return Err(UiError::DocInvalid {
                                node: state.path.clone(),
                                reason: "slider binding is outside its declared range",
                            });
                        }
                    }
                    Content::Choice {
                        options, selected, ..
                    } => {
                        let list = list_binding_in(
                            self.bindings,
                            options,
                            &state.path,
                            state.row.as_ref(),
                        )?;
                        let selected = number_binding(
                            self.bindings,
                            selected,
                            &state.path,
                            state.row.as_ref(),
                        )?;
                        if index_from_f32(selected)
                            .is_none_or(|index| usize::from(index) >= list.records().len())
                        {
                            return Err(UiError::DocInvalid {
                                node: state.path.clone(),
                                reason: "choice selection is outside the options list",
                            });
                        }
                    }
                    _ => {}
                }
                let is_disabled = match disabled {
                    Some(name) => {
                        flag_binding(self.bindings, name, &state.path, state.row.as_ref())?
                    }
                    None => false,
                };
                let Some(widget_index) = self.widget_states.iter().position(|(name, row, _)| {
                    name == id && *row == state.row.as_ref().map(|row| row.index)
                }) else {
                    return Err(UiError::Layout(
                        "widget state table is incomplete".to_owned(),
                    ));
                };
                self.out.widgets.push(WidgetRegion {
                    widget_index,
                    row: state.row.as_ref().map(|row| row.index),
                    rect: paint_rect,
                    skew,
                    disabled: is_disabled,
                });
                let color = text_color
                    .map(|fill| Paint::of(fill, opacity))
                    .unwrap_or(Paint {
                        rgb: crate::color::Srgb8 {
                            r: 255,
                            g: 255,
                            b: 255,
                        },
                        alpha: opacity,
                    });
                let start = self.out.text.len();
                self.out.text.push_str(&state.text);
                if let Content::Choice {
                    options, selected, ..
                } = &node.content
                {
                    self.out.text.push_str(": ");
                    self.out.text.push_str(selected_label(
                        self.bindings,
                        options,
                        selected,
                        &state.path,
                        state.row.as_ref(),
                    )?);
                }
                fit_text(
                    self.measure,
                    self.ellipsis_scratch,
                    self.out,
                    state,
                    TextRunSpec {
                        start,
                        rect: paint_rect,
                        parent: parent_rect,
                        base_px: 20.0 * self.scale,
                        color,
                        align: TextAlign::Center,
                        node_index: current,
                        fit: node.fit,
                        overflow: node.text_overflow,
                    },
                )?;
            }
            Content::Tabs {
                id,
                tabs,
                selected,
                disabled,
            } => {
                let list = list_binding_in(self.bindings, tabs, &state.path, state.row.as_ref())?;
                let index =
                    number_binding(self.bindings, selected, &state.path, state.row.as_ref())?;
                if index_from_f32(index)
                    .is_none_or(|index| usize::from(index) >= list.records().len())
                {
                    return Err(UiError::DocInvalid {
                        node: state.path.clone(),
                        reason: "tab selection is outside the tabs list",
                    });
                }
                let is_disabled = match disabled {
                    Some(name) => {
                        flag_binding(self.bindings, name, &state.path, state.row.as_ref())?
                    }
                    None => false,
                };
                let Some(widget_index) = self.widget_states.iter().position(|(name, row, _)| {
                    name == id && *row == state.row.as_ref().map(|row| row.index)
                }) else {
                    return Err(UiError::Layout(
                        "widget state table is incomplete".to_owned(),
                    ));
                };
                self.out.widgets.push(WidgetRegion {
                    widget_index,
                    row: state.row.as_ref().map(|row| row.index),
                    rect: paint_rect,
                    skew,
                    disabled: is_disabled,
                });
                let start = self.out.text.len();
                self.out.text.push_str(selected_label(
                    self.bindings,
                    tabs,
                    selected,
                    &state.path,
                    state.row.as_ref(),
                )?);
                fit_text(
                    self.measure,
                    self.ellipsis_scratch,
                    self.out,
                    state,
                    TextRunSpec {
                        start,
                        rect: paint_rect,
                        parent: parent_rect,
                        base_px: 20.0 * self.scale,
                        color: Paint::of(
                            text_color.unwrap_or(crate::doc::Color {
                                rgb: crate::color::Srgb8 {
                                    r: 255,
                                    g: 255,
                                    b: 255,
                                },
                                a: 255,
                            }),
                            opacity,
                        ),
                        align: TextAlign::Center,
                        node_index: current,
                        fit: node.fit,
                        overflow: node.text_overflow,
                    },
                )?;
            }
            Content::Repeat { .. } => {}
        }
        for child in &node.children {
            self.apply(child, index, (rect.x, rect.y), rect, opacity, skew)?;
        }
        if skew != outer_skew {
            self.out.prims.push(Primitive::Skew(outer_skew));
        }
        Ok(())
    }
}
