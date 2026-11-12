//! Repeat expansion, stable source identities and layout rectangle traversal.

use std::sync::Arc;

use engine_core::Rect;
use taffy::prelude as t;

use super::runtime::{Dependency, ExpandedNode, Impact, NodeRectContext, NodeState, RowBinding};
use super::text::layout_err;
use super::update::text_content;
use crate::doc::{Content, Direction, Display, Node};
use crate::error::UiError;
use crate::focus::RowIndex;
use crate::layout::{NodeRect, WidgetVisualState};

pub(crate) fn expand_node(
    node: &Node,
    row: Option<RowBinding>,
    parent_direction: Direction,
    parent_gap: f32,
) -> Result<ExpandedNode, UiError> {
    let mut expanded_node = node.clone();
    if let Some(row) = &row
        && let Some(source) = &mut expanded_node.source
        && let Some(suffix) = source.strip_prefix(row.source_root.as_ref())
    {
        *source = format!("{}[{}]{suffix}", row.source_root, row.index.0);
    }
    let mut expanded = ExpandedNode {
        node: expanded_node,
        children: Vec::new(),
        row,
    };
    if let Content::Repeat {
        source,
        max,
        template,
    } = &node.content
    {
        expanded.children.reserve(usize::from(*max));
        if node.layout.display == Display::Block {
            expanded.node.layout.display = Display::Flex;
            expanded.node.layout.direction = parent_direction;
            expanded.node.layout.gap = parent_gap;
        }
        let source: Arc<str> = Arc::from(source.as_str());
        let source_root: Arc<str> = Arc::from(node.source.as_deref().unwrap_or_default());
        for index in 0..usize::from(*max) {
            let index = u16::try_from(index).map_err(|_| UiError::IdExhausted)?;
            expanded.children.push(expand_node(
                template,
                Some(RowBinding {
                    source: Arc::clone(&source),
                    source_root: Arc::clone(&source_root),
                    index: RowIndex(index),
                }),
                node.layout.direction,
                0.0,
            )?);
        }
    } else {
        expanded.children.reserve(node.children.len());
        for child in &node.children {
            expanded.children.push(expand_node(
                child,
                expanded.row.clone(),
                node.layout.direction,
                node.layout.gap,
            )?);
        }
    }
    Ok(expanded)
}

pub(crate) fn node_count(node: &ExpandedNode) -> usize {
    node.children.iter().fold(1usize, |count, child| {
        count.saturating_add(node_count(child))
    })
}

pub(crate) fn collect_node_rects(
    node: &ExpandedNode,
    states: &[NodeState],
    tree: &t::TaffyTree<()>,
    index: &mut usize,
    context: NodeRectContext,
    out: &mut Vec<NodeRect>,
) -> Result<(), UiError> {
    let state = states
        .get(*index)
        .ok_or_else(|| UiError::Layout("node map is incomplete".to_owned()))?;
    *index += 1;
    let layout = tree.layout(state.id).map_err(layout_err)?;
    let rect = Rect {
        x: context.origin.0 + layout.location.x,
        y: context.origin.1 + layout.location.y,
        w: layout.size.width,
        h: layout.size.height,
    };
    // CSS transforms affect the browser's bounding client rect, so expose the same visual
    // bounds while retaining the untransformed box for positioning descendants.
    let own_slope = node.style.skew.to_radians().tan();
    let pivot_y = rect.y + rect.h * 0.5;
    let slope = context.skew.0 + own_slope;
    let offset = context.skew.1 - own_slope * pivot_y;
    let top_x = rect.x + slope * rect.y + offset;
    let bottom_x = rect.x + slope * (rect.y + rect.h) + offset;
    let visual_rect = Rect {
        x: top_x.min(bottom_x),
        y: rect.y,
        w: rect.w + (bottom_x - top_x).abs(),
        h: rect.h,
    };
    let visible = context.visible && state.visible;
    out.push(NodeRect {
        source: state.path.clone(),
        rect: visual_rect,
        visible,
        visible_content: match &node.content {
            Content::Text { .. } => !state.text.is_empty(),
            Content::Bar { .. }
            | Content::Button { .. }
            | Content::Action { .. }
            | Content::Toggle { .. }
            | Content::Slider { .. }
            | Content::Choice { .. }
            | Content::Tabs { .. } => true,
            Content::None | Content::Repeat { .. } => false,
        },
    });
    for child in &node.children {
        collect_node_rects(
            child,
            states,
            tree,
            index,
            NodeRectContext {
                origin: (rect.x, rect.y),
                visible,
                skew: (slope, offset),
            },
            out,
        )?;
    }
    Ok(())
}

pub(crate) fn widget_id(content: &Content) -> Option<&str> {
    match content {
        Content::Action { id, .. }
        | Content::Toggle { id, .. }
        | Content::Slider { id, .. }
        | Content::Choice { id, .. }
        | Content::Tabs { id, .. } => Some(id),
        _ => None,
    }
}

pub(crate) fn collect_widget_states(
    node: &ExpandedNode,
    out: &mut Vec<(String, Option<RowIndex>, WidgetVisualState)>,
) {
    if let Some(id) = widget_id(&node.content) {
        out.push((
            id.to_owned(),
            node.row.as_ref().map(|row| row.index),
            WidgetVisualState::default(),
        ));
    }
    for child in &node.children {
        collect_widget_states(child, out);
    }
}

pub(crate) const REPEATED_TEXT_CAPACITY: usize = 256;

pub(crate) fn text_capacity(node: &ExpandedNode) -> usize {
    let source_capacity = match &node.content {
        Content::Text { text, .. } => text.source().len(),
        Content::Button { label, .. } => label.len(),
        Content::Action { label, .. }
        | Content::Toggle { label, .. }
        | Content::Slider { label, .. }
        | Content::Choice { label, .. } => label.source().len(),
        _ => 0,
    };
    let own = if node.row.is_some() && text_content(&node.node).is_some() {
        source_capacity.max(REPEATED_TEXT_CAPACITY)
    } else {
        source_capacity
    };
    node.children.iter().fold(own, |count, child| {
        count.saturating_add(text_capacity(child))
    })
}

pub(crate) fn intern_dependency(deps: &mut Vec<Dependency>, name: &str, impact: Impact) -> usize {
    if let Some(index) = deps.iter().position(|dep| dep.name == name) {
        if impact == Impact::Layout
            && let Some(dep) = deps.get_mut(index)
        {
            dep.impact = Impact::Layout;
        }
        return index;
    }
    let index = deps.len();
    deps.push(Dependency {
        name: name.to_owned(),
        impact,
        generation: 0,
    });
    index
}
