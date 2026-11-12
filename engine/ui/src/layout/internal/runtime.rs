//! Public retained layout lifecycle and dirty-frame orchestration.

use std::sync::Arc;

use engine_core::{Rect, Viewport};
use taffy::prelude as t;

use crate::bindings::Bindings;
use crate::doc::{Direction, Document, Node};
use crate::error::UiError;
use crate::focus::RowIndex;

use super::draw::DrawContext;
use super::repeat::{
    collect_node_rects, collect_widget_states, expand_node, node_count, text_capacity,
};
use super::text::layout_err;
use super::tree::build_tree;
use super::update::UpdateContext;
use crate::layout::{
    DrawList, MetricsKey, NodeRect, REFERENCE_SHORT_SIDE, TextMeasure, WidgetRegion,
    WidgetVisualState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Impact {
    Layout,
    Paint,
}

#[derive(Debug)]
pub(crate) struct Dependency {
    pub(crate) name: String,
    pub(crate) impact: Impact,
    pub(crate) generation: u64,
}

#[derive(Debug)]
pub(crate) struct NodeState {
    pub(crate) id: t::NodeId,
    pub(crate) path: String,
    pub(crate) dependencies: Vec<(usize, Impact)>,
    pub(crate) text: String,
    pub(crate) natural: Option<(f32, f32)>,
    pub(crate) fitted_width: Option<(f32, f32)>,
    pub(crate) visible: bool,
    pub(crate) initialized: bool,
    pub(crate) row: Option<RowBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RowBinding {
    pub(crate) source: Arc<str>,
    pub(crate) source_root: Arc<str>,
    pub(crate) index: RowIndex,
}

#[derive(Clone, Copy)]
pub(crate) struct NodeRectContext {
    pub(crate) origin: (f32, f32),
    pub(crate) visible: bool,
    pub(crate) skew: (f32, f32),
}

#[derive(Debug)]
pub(crate) struct ExpandedNode {
    pub(crate) node: Node,
    pub(crate) children: Vec<ExpandedNode>,
    pub(crate) row: Option<RowBinding>,
}

impl std::ops::Deref for ExpandedNode {
    type Target = Node;

    fn deref(&self) -> &Self::Target {
        &self.node
    }
}

/// Retained layout state. The document, taffy tree and stable preorder node map share one lifetime.
#[derive(Debug)]
pub struct UiLayout {
    document: Document,
    layout_root: ExpandedNode,
    tree: t::TaffyTree<()>,
    root: t::NodeId,
    states: Vec<NodeState>,
    dependencies: Vec<Dependency>,
    changed: Vec<bool>,
    out: DrawList,
    scratch_out: DrawList,
    ellipsis_scratch: String,
    screen: Option<(f32, f32)>,
    metrics: Option<MetricsKey>,
    last_binding_generation: u64,
    widget_states: Vec<(String, Option<RowIndex>, WidgetVisualState)>,
    widgets_dirty: bool,
}

impl UiLayout {
    /// Builds a retained layout for `document`.
    ///
    /// # Errors
    ///
    /// Returns [`UiError::Layout`] if taffy rejects the tree.
    pub fn new(document: Document) -> Result<Self, UiError> {
        let layout_root = expand_node(document.root(), None, Direction::Row, 0.0)?;
        let count = node_count(&layout_root);
        let mut widget_states = Vec::new();
        collect_widget_states(&layout_root, &mut widget_states);
        let mut tree = t::TaffyTree::new();
        let mut states = Vec::with_capacity(count);
        let mut dependencies = Vec::with_capacity(count);
        let root = build_tree(
            &layout_root,
            "root",
            &mut tree,
            &mut states,
            &mut dependencies,
        )?;
        let mut out = DrawList::default();
        let mut scratch_out = DrawList::default();
        for draw_list in [&mut out, &mut scratch_out] {
            draw_list.prims.reserve(count.saturating_mul(8));
            draw_list.touch_regions.reserve(count);
            draw_list.touch_bounds.reserve(count);
            draw_list.widgets.reserve(count);
            draw_list.text_runs.reserve(count);
        }
        let text_capacity = text_capacity(&layout_root);
        out.text.reserve(text_capacity);
        scratch_out.text.reserve(text_capacity);
        let changed = vec![false; dependencies.len()];
        Ok(Self {
            document,
            layout_root,
            tree,
            root,
            states,
            dependencies,
            changed,
            out,
            scratch_out,
            ellipsis_scratch: String::with_capacity(text_capacity.saturating_add(3)),
            screen: None,
            metrics: None,
            last_binding_generation: 0,
            widget_states,
            widgets_dirty: false,
        })
    }

    /// Replaces the document and all retained state after a successful hot reload.
    ///
    /// If building the replacement fails, the current document and layout remain unchanged.
    pub fn reload(&mut self, document: Document) -> Result<(), UiError> {
        *self = Self::new(document)?;
        Ok(())
    }

    /// The document retained by this layout.
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Cached menu-widget hit regions from the most recent layout frame.
    #[must_use]
    pub fn draw_list_widgets(&self) -> &[WidgetRegion] {
        self.out.widgets()
    }

    /// Cached draw list, or `None` until the first frame has been laid out.
    #[must_use]
    pub fn draw_list_ref(&self) -> Option<&DrawList> {
        self.screen.map(|_| &self.out)
    }

    /// Returns the current rectangle of every node, including unpainted containers.
    pub fn node_rects(&self) -> Result<Vec<NodeRect>, UiError> {
        let mut rects = Vec::with_capacity(self.states.len());
        if self.screen.is_some() {
            let mut index = 0;
            collect_node_rects(
                &self.layout_root,
                &self.states,
                &self.tree,
                &mut index,
                NodeRectContext {
                    origin: (0.0, 0.0),
                    visible: true,
                    skew: (0.0, 0.0),
                },
                &mut rects,
            )?;
        }
        Ok(rects)
    }

    /// Rejects any visible text or widget whose laid out box has no area.
    pub fn check_visible_content(&self) -> Result<(), UiError> {
        for node in self.node_rects()? {
            if node.visible_content
                && node.visible
                && (!node.rect.w.is_finite()
                    || !node.rect.h.is_finite()
                    || node.rect.w <= 0.0
                    || node.rect.h <= 0.0)
            {
                return Err(UiError::ZeroSizeContent { node: node.source });
            }
        }
        Ok(())
    }

    /// Updates a widget's state style, returning whether the cached list needs repainting.
    pub fn set_widget_state(&mut self, widget_index: usize, state: WidgetVisualState) -> bool {
        if let Some((_, _, current)) = self.widget_states.get_mut(widget_index)
            && *current != state
        {
            *current = state;
            self.widgets_dirty = true;
        }
        self.widgets_dirty
    }

    /// Lays out the retained document for a screen in pixels, returning the cached list if its
    /// bindings, screen and text metrics are unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`UiError::UnboundVariable`] or [`UiError::BindingType`] for invalid bindings and
    /// [`UiError::Layout`] if taffy rejects the tree.
    pub fn frame(
        &mut self,
        bindings: &Bindings,
        screen: (f32, f32),
        measure: &mut dyn TextMeasure,
    ) -> Result<&DrawList, UiError> {
        let (sw, sh) = screen;
        if !(sw.is_finite() && sh.is_finite() && sw > 0.0 && sh > 0.0) {
            return Err(UiError::Layout(
                "screen size must be finite and positive".to_owned(),
            ));
        }
        let metrics = measure.metrics_key();
        let screen_changed = self.screen != Some(screen);
        let metrics_changed = self.metrics != Some(metrics);
        let binding_generation = bindings.global_generation();
        if !screen_changed
            && !metrics_changed
            && binding_generation == self.last_binding_generation
            && !self.widgets_dirty
        {
            return Ok(&self.out);
        }
        let short = sw.min(sh);
        let previous_short = self.screen.map(|(w, h)| w.min(h));
        let scale_changed = previous_short != Some(short);
        let mut layout_changed = screen_changed || metrics_changed;
        let mut paint_changed = false;
        for (index, dep) in self.dependencies.iter().enumerate() {
            let changed = bindings.generation(&dep.name) != dep.generation;
            if let Some(slot) = self.changed.get_mut(index) {
                *slot = changed;
            }
            if changed {
                match dep.impact {
                    Impact::Layout => layout_changed = true,
                    Impact::Paint => paint_changed = true,
                }
            }
        }
        if !layout_changed && !paint_changed && !self.widgets_dirty {
            self.last_binding_generation = binding_generation;
            return Ok(&self.out);
        }
        let scale = short / REFERENCE_SHORT_SIDE;
        if layout_changed || paint_changed {
            let tree = &mut self.tree;
            let mut index = 0;
            let mut updater = UpdateContext {
                states: &mut self.states,
                changed: &self.changed,
                tree,
                bindings,
                measure,
                scale,
                short,
                remeasure_all: scale_changed || metrics_changed,
            };
            let mut geometry_dirty = updater.apply(&self.layout_root, &mut index)?;
            let root = self.root;
            if screen_changed {
                let mut root_style = tree.style(root).map_err(layout_err)?.clone();
                root_style.size = taffy::Size {
                    width: t::length(sw),
                    height: t::length(sh),
                };
                tree.set_style(root, root_style).map_err(layout_err)?;
                tree.mark_dirty(root).map_err(layout_err)?;
                geometry_dirty = true;
            }
            if geometry_dirty {
                tree.compute_layout(
                    root,
                    taffy::Size {
                        width: taffy::AvailableSpace::Definite(sw),
                        height: taffy::AvailableSpace::Definite(sh),
                    },
                )
                .map_err(layout_err)?;
            }
        }
        self.scratch_out.clear();
        let tree = &self.tree;
        let mut index = 0;
        let mut drawer = DrawContext {
            states: &mut self.states,
            widget_states: &self.widget_states,
            tree,
            viewport: Viewport::new(screen.0, screen.1).ok_or_else(|| {
                UiError::Layout("screen size must be finite and positive".to_owned())
            })?,
            scale,
            bindings,
            measure,
            ellipsis_scratch: &mut self.ellipsis_scratch,
            out: &mut self.scratch_out,
        };
        let draw_result = drawer.apply(
            &self.layout_root,
            &mut index,
            (0.0, 0.0),
            Rect {
                x: 0.0,
                y: 0.0,
                w: screen.0,
                h: screen.1,
            },
            1.0,
            None,
        );
        if let Err(error) = draw_result {
            self.scratch_out.clear();
            return Err(error);
        }
        std::mem::swap(&mut self.out, &mut self.scratch_out);
        self.widgets_dirty = false;
        for dependency in &mut self.dependencies {
            dependency.generation = bindings.generation(&dependency.name);
        }
        self.screen = Some(screen);
        self.metrics = Some(metrics);
        self.last_binding_generation = binding_generation;
        Ok(&self.out)
    }
}
