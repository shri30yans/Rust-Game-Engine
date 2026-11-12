//! Menu stack lifecycle, retained screen updates and input dispatch.

use engine_core::Rect;
use glam::Vec2;

use crate::bindings::Bindings;
use crate::doc::{BackRule, Document};
use crate::error::UiError;
use crate::focus::{FocusNav, FocusRect, NavInput, RowIndex, WidgetId};
use crate::layout::{TextMeasure, UiLayout, WidgetVisualState};
use crate::stack::{Layer, ScreenId, ScreenStack};

use super::focus::{
    contains_widget, fill_focus_rects, gather_widget_handles, restored_visible_focus, wrap_focus,
};
use super::input::{
    INITIAL_EVENT_CAPACITY, MenuDraw, MenuInput, MenuMotion, MenuTouchPhase, REDUCED_MOTION_MS,
    Transition, UiEvent,
};
use super::transition::{layer_order, update_transition};
use super::widgets::{activate_widget, adjust_selected_widget, adjust_widget, is_tabs_widget};

pub(super) struct OpenScreen {
    pub(super) id: ScreenId,
    pub(super) layer: Layer,
    pub(super) layout: UiLayout,
    pub(super) focus_nav: FocusNav,
    pub(super) focus: Option<WidgetId>,
    pub(super) transition: Transition,
    pub(super) started_ms: u64,
    pub(super) duration_ms: u32,
    pub(super) curve: (f32, f32, f32, f32),
    pub(super) exiting: bool,
    pub(super) exit_started_ms: u64,
    pub(super) covered: bool,
    pub(super) cover_started_ms: u64,
    pub(super) resuming: bool,
    pub(super) resume_started_ms: u64,
    pub(super) back_action: Option<WidgetId>,
    pub(super) widget_handles: Vec<WidgetInstance>,
    pub(super) base_widget_handles: Vec<(String, WidgetId)>,
    pub(super) focus_rects: Vec<FocusRect>,
}

pub(super) struct WidgetInstance {
    pub(super) name: String,
    pub(super) base_id: WidgetId,
    pub(super) focus_id: WidgetId,
    pub(super) row: Option<RowIndex>,
    pub(super) row_source: Option<String>,
}

struct ApproxMeasure;
impl TextMeasure for ApproxMeasure {
    fn metrics_key(&self) -> crate::layout::MetricsKey {
        crate::layout::MetricsKey(0)
    }
    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * 0.5 * f32::from(u16::try_from(text.chars().count()).unwrap_or(u16::MAX))
    }
}

/// Engine menu runtime over retained UI documents.
pub struct Menus {
    stack: ScreenStack,
    screens: Vec<OpenScreen>,
    events: Vec<UiEvent>,
    motion: MenuMotion,
    next_screen: u32,
    next_widget: u32,
    reduce_motion: bool,
    now_ms: u64,
    last_pointer: Option<Vec2>,
}

impl Menus {
    /// Creates an empty menu runtime with its transition configuration.
    #[must_use]
    pub fn new(motion: MenuMotion) -> Self {
        Self {
            stack: ScreenStack::new(),
            screens: Vec::new(),
            events: Vec::with_capacity(INITIAL_EVENT_CAPACITY),
            motion,
            next_screen: 1,
            next_widget: 1,
            reduce_motion: false,
            now_ms: 0,
            last_pointer: None,
        }
    }

    /// Enables the accessibility reduced-motion transition.
    pub fn set_reduce_motion(&mut self, reduce: bool) {
        self.reduce_motion = reduce;
    }

    /// Resolves a document widget name to the stable typed handle assigned at screen load.
    #[must_use]
    pub fn widget_id(&self, screen: ScreenId, name: &str) -> Option<WidgetId> {
        self.screens
            .iter()
            .find(|entry| entry.id == screen)?
            .base_widget_handles
            .iter()
            .find(|(id, _)| id == name)
            .map(|(_, handle)| *handle)
    }

    /// Visible bounds for every instance of a named widget on one screen.
    #[must_use]
    pub fn widget_rects(&self, screen_id: ScreenId, name: &str) -> Vec<Rect> {
        let Some(screen) = self.screens.iter().find(|screen| screen.id == screen_id) else {
            return Vec::new();
        };
        let regions = screen.layout.draw_list_widgets();
        screen
            .widget_handles
            .iter()
            .enumerate()
            .filter(|(_, widget)| widget.name == name)
            .filter_map(|(index, _)| regions.iter().find(|region| region.widget_index == index))
            .map(|region| region.rect)
            .collect()
    }

    /// Activates a document on the specified layer.
    pub fn push(&mut self, layer: Layer, document: Document) -> Result<ScreenId, UiError> {
        let id = ScreenId(self.next_screen);
        self.next_screen = self
            .next_screen
            .checked_add(1)
            .ok_or(UiError::IdExhausted)?;
        let mut handles = Vec::new();
        let mut base_handles = Vec::new();
        gather_widget_handles(
            document.root(),
            None,
            &mut handles,
            &mut base_handles,
            &mut self.next_widget,
        )?;
        let back_action = document.screen().and_then(|screen| match &screen.back {
            BackRule::Action(name) => handles
                .iter()
                .find(|widget| widget.name == *name)
                .map(|widget| widget.base_id),
            BackRule::Pop => None,
        });
        let default = document
            .screen()
            .and_then(|screen| screen.default_focus.as_deref());
        let first = handles.first().map(|widget| widget.focus_id);
        let focus = default
            .and_then(|name| {
                handles
                    .iter()
                    .find(|widget| widget.name == name)
                    .map(|widget| widget.focus_id)
            })
            .or(first);
        let count = handles.len();
        let layout = UiLayout::new(document)?;
        let (duration_ms, curve) = if self.reduce_motion {
            (REDUCED_MOTION_MS, (0.0, 0.0, 1.0, 1.0))
        } else if layer == Layer::Modal {
            (self.motion.modal.ms, self.motion.modal.curve)
        } else {
            (self.motion.screen.ms, self.motion.screen.curve)
        };
        if layer.accepts_focus()
            && let Some((_, previous_id)) = self.stack.focused()
            && let Some(previous) = self
                .screens
                .iter_mut()
                .find(|screen| screen.id == previous_id)
        {
            previous.covered = true;
            previous.cover_started_ms = self.now_ms;
        }
        let insert_at = self
            .screens
            .iter()
            .rposition(|screen| layer_order(screen.layer) <= layer_order(layer))
            .map_or(0, |index| index + 1);
        self.screens.insert(
            insert_at,
            OpenScreen {
                id,
                layer,
                layout,
                focus_nav: FocusNav::with_capacity(count),
                focus,
                transition: Transition {
                    opacity: 0.0,
                    offset_x: self.motion.slide_px,
                    scale: if layer == Layer::Modal { 0.96 } else { 1.0 },
                },
                started_ms: self.now_ms,
                duration_ms,
                curve,
                exiting: false,
                exit_started_ms: 0,
                covered: false,
                cover_started_ms: 0,
                resuming: false,
                resume_started_ms: 0,
                back_action,
                widget_handles: handles,
                base_widget_handles: base_handles,
                focus_rects: Vec::with_capacity(count),
            },
        );
        self.stack.push(layer, id);
        Ok(id)
    }

    /// Removes the top screen in `layer`, keeping it drawable through its exit fade.
    pub fn pop(&mut self, layer: Layer) -> Result<ScreenId, UiError> {
        let id = self.stack.pop_layer(layer)?;
        if let Some(screen) = self.screens.iter_mut().find(|screen| screen.id == id) {
            screen.exiting = true;
            screen.exit_started_ms = self.now_ms;
        }
        if let Some((_, resumed_id)) = self.stack.focused()
            && let Some(resumed) = self
                .screens
                .iter_mut()
                .find(|screen| screen.id == resumed_id)
        {
            resumed.covered = false;
            resumed.resuming = true;
            resumed.resume_started_ms = self.now_ms;
        }
        Ok(id)
    }

    /// Replaces the top screen in `layer` with a newly activated screen.
    pub fn replace(&mut self, layer: Layer, document: Document) -> Result<ScreenId, UiError> {
        if self.stack.top(layer).is_some() {
            self.pop(layer)?;
        }
        self.push(layer, document)
    }

    /// Routes one frame to the active screen and returns the events produced by that frame.
    pub fn frame(
        &mut self,
        input: &MenuInput,
        bindings: &Bindings,
        screen_size: (f32, f32),
        now_ms: u64,
    ) -> Result<&[UiEvent], UiError> {
        self.now_ms = now_ms;
        self.events.clear();
        let pointer_moved = self
            .last_pointer
            .is_some_and(|previous| previous != input.pointer.position);
        self.last_pointer = Some(input.pointer.position);
        for screen in &mut self.screens {
            if !screen.exiting {
                screen
                    .layout
                    .frame(bindings, screen_size, &mut ApproxMeasure)?;
            }
            update_transition(
                screen,
                now_ms,
                self.reduce_motion,
                self.motion.exit_ms,
                self.motion.slide_px,
            );
        }
        let Some((layer, id)) = self.stack.focused() else {
            self.collect_finished_exits();
            return Ok(&self.events);
        };
        let mut should_pop = false;
        if let Some(screen) = self.screens.iter_mut().find(|screen| screen.id == id) {
            let regions = screen.layout.draw_list_widgets();
            fill_focus_rects(regions, &screen.widget_handles, &mut screen.focus_rects);
            if screen
                .focus
                .is_none_or(|focus| !screen.focus_rects.iter().any(|rect| rect.id == focus))
            {
                screen.focus = restored_visible_focus(screen, screen.focus);
            }
            let mut hover = None;
            let mut pressed = None;
            let mut navigation = input.nav;
            if let Some(direction @ (NavInput::Left | NavInput::Right)) = navigation
                && let Some(focus) = screen.focus
                && adjust_selected_widget(
                    screen,
                    focus,
                    direction == NavInput::Right,
                    bindings,
                    &mut self.events,
                )?
            {
                navigation = None;
            }
            if navigation.is_none()
                && input.scroll != 0.0
                && let Some(focus) = screen.focus
            {
                let _ = adjust_selected_widget(
                    screen,
                    focus,
                    input.scroll < 0.0,
                    bindings,
                    &mut self.events,
                )?;
            }
            if let Some(direction) = navigation
                && let Some(current) = screen.focus
            {
                let candidate =
                    screen
                        .focus_nav
                        .navigate(current, direction, &screen.focus_rects, &[]);
                screen.focus =
                    candidate.or_else(|| wrap_focus(current, direction, &screen.focus_rects));
            }
            {
                if let Some(region) = regions.iter().find(|region| {
                    !region.disabled && contains_widget(region, input.pointer.position)
                }) {
                    hover = screen
                        .widget_handles
                        .get(region.widget_index)
                        .map(|widget| widget.focus_id);
                    if pointer_moved || input.pointer.pressed {
                        screen.focus = hover;
                    }
                    if input.pointer.down {
                        pressed = hover;
                    }
                    if input.pointer.pressed {
                        activate_widget(screen, region, input, bindings, &mut self.events)?;
                    }
                }
            }
            for touch in input.touches() {
                if touch.phase != MenuTouchPhase::Ended
                    && let Some(region) = regions
                        .iter()
                        .find(|region| !region.disabled && contains_widget(region, touch.position))
                {
                    pressed = screen
                        .widget_handles
                        .get(region.widget_index)
                        .map(|widget| widget.focus_id);
                    if touch.phase == MenuTouchPhase::Began {
                        activate_widget(screen, region, input, bindings, &mut self.events)?;
                    }
                }
            }
            if input.back {
                if let Some(action) = screen.back_action {
                    self.events.push(UiEvent::Action(action, None));
                } else {
                    self.events.push(UiEvent::Back(id));
                    should_pop = true;
                }
            } else if input.confirm
                && let Some(focus) = screen.focus
                && let Some(region) = regions.iter().find(|region| {
                    screen
                        .widget_handles
                        .get(region.widget_index)
                        .is_some_and(|handle| handle.focus_id == focus)
                })
            {
                activate_widget(screen, region, input, bindings, &mut self.events)?;
            }
            if input.tab_prev || input.tab_next {
                let adjusted = screen
                    .focus
                    .is_some_and(|focus| is_tabs_widget(screen, focus));
                if adjusted {
                    if let Some(focus) = screen.focus {
                        let forward = input.tab_next;
                        if let Some(region) = regions.iter().find(|region| {
                            screen
                                .widget_handles
                                .get(region.widget_index)
                                .is_some_and(|handle| handle.focus_id == focus)
                        }) {
                            adjust_widget(screen, region, forward, bindings, &mut self.events)?;
                        }
                    }
                } else {
                    let direction = if input.tab_prev {
                        NavInput::Prev
                    } else {
                        NavInput::Next
                    };
                    if let Some(current) = screen.focus {
                        screen.focus = screen
                            .focus_nav
                            .navigate(current, direction, &screen.focus_rects, &[])
                            .or_else(|| wrap_focus(current, direction, &screen.focus_rects));
                    }
                }
            }
            for (widget_index, handle) in screen.widget_handles.iter().enumerate() {
                let disabled = screen
                    .layout
                    .draw_list_widgets()
                    .get(widget_index)
                    .is_some_and(|region| region.disabled);
                screen.layout.set_widget_state(
                    widget_index,
                    WidgetVisualState {
                        hovered: Some(handle.focus_id) == hover,
                        focused: Some(handle.focus_id) == screen.focus,
                        pressed: Some(handle.focus_id) == pressed,
                        disabled,
                    },
                );
            }
            screen
                .layout
                .frame(bindings, screen_size, &mut ApproxMeasure)?;
        }
        if should_pop {
            self.pop(layer)?;
        }
        self.collect_finished_exits();
        Ok(&self.events)
    }

    /// Drawable screens in painter order, including screens finishing their exit transition.
    pub fn draw_list(&self) -> impl Iterator<Item = MenuDraw<'_>> {
        self.screens.iter().filter_map(|screen| {
            Some(MenuDraw {
                screen: screen.id,
                layer: screen.layer,
                list: screen.layout.draw_list_ref()?,
                transition: screen.transition,
            })
        })
    }

    /// Checks each retained screen's visible content boxes after a layout frame.
    pub fn check_visible_content(&self) -> Result<(), UiError> {
        for screen in &self.screens {
            screen.layout.check_visible_content()?;
        }
        Ok(())
    }

    fn collect_finished_exits(&mut self) {
        let duration = if self.reduce_motion {
            REDUCED_MOTION_MS
        } else {
            self.motion.exit_ms
        };
        let now = self.now_ms;
        self.screens.retain(|screen| {
            !screen.exiting || now.saturating_sub(screen.exit_started_ms) < u64::from(duration)
        });
    }
}
