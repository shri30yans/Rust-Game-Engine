//! Taffy node construction and binding dependency registration.

use taffy::prelude as t;

use super::repeat::{REPEATED_TEXT_CAPACITY, intern_dependency};
use super::runtime::{Dependency, ExpandedNode, Impact, NodeState};
use super::text::layout_err;
use super::update::split_condition;
use crate::doc::Content;
use crate::error::UiError;

pub(crate) fn build_tree(
    node: &ExpandedNode,
    path: &str,
    tree: &mut t::TaffyTree<()>,
    states: &mut Vec<NodeState>,
    deps: &mut Vec<Dependency>,
) -> Result<t::NodeId, UiError> {
    let id = tree.new_leaf(t::Style::default()).map_err(layout_err)?;
    let state_index = states.len();
    states.push(NodeState {
        id,
        path: node.source.clone().unwrap_or_else(|| path.to_owned()),
        dependencies: Vec::new(),
        text: String::new(),
        natural: None,
        fitted_width: None,
        visible: false,
        initialized: false,
        row: node.row.clone(),
    });
    let mut node_deps = Vec::new();
    if let Some(condition) = &node.show_if {
        let (name, _) = split_condition(condition);
        if !name.starts_with("item.") {
            node_deps.push((
                intern_dependency(deps, name, Impact::Layout),
                Impact::Layout,
            ));
        }
    }
    if let Some(row) = &node.row {
        node_deps.push((
            intern_dependency(deps, &row.source, Impact::Paint),
            Impact::Paint,
        ));
    }
    match &node.content {
        Content::Text { text, .. } => {
            for name in text.variables().filter(|name| !name.starts_with("item.")) {
                node_deps.push((
                    intern_dependency(deps, name, Impact::Layout),
                    Impact::Layout,
                ));
            }
        }
        Content::Bar { value, .. } => {
            node_deps.push((intern_dependency(deps, value, Impact::Paint), Impact::Paint))
        }
        Content::Button { held, .. } => {
            node_deps.push((intern_dependency(deps, held, Impact::Paint), Impact::Paint))
        }
        Content::Action {
            label, disabled, ..
        }
        | Content::Toggle {
            label, disabled, ..
        }
        | Content::Slider {
            label, disabled, ..
        }
        | Content::Choice {
            label, disabled, ..
        } => {
            for name in label.variables().filter(|name| !name.starts_with("item.")) {
                node_deps.push((
                    intern_dependency(deps, name, Impact::Layout),
                    Impact::Layout,
                ));
            }
            if let Some(name) = disabled {
                node_deps.push((intern_dependency(deps, name, Impact::Paint), Impact::Paint));
            }
            match &node.content {
                Content::Toggle { value, .. } | Content::Slider { value, .. } => {
                    node_deps.push((intern_dependency(deps, value, Impact::Paint), Impact::Paint))
                }
                Content::Choice {
                    options, selected, ..
                } => {
                    for name in [options, selected] {
                        node_deps
                            .push((intern_dependency(deps, name, Impact::Paint), Impact::Paint));
                    }
                }
                _ => {}
            }
        }
        Content::Tabs {
            tabs,
            selected,
            disabled,
            ..
        } => {
            for name in [tabs, selected] {
                node_deps.push((intern_dependency(deps, name, Impact::Paint), Impact::Paint));
            }
            if let Some(name) = disabled {
                node_deps.push((intern_dependency(deps, name, Impact::Paint), Impact::Paint));
            }
        }
        Content::Repeat { source, .. } => node_deps.push((
            intern_dependency(deps, source, Impact::Paint),
            Impact::Paint,
        )),
        Content::None => {}
    }
    if let Some(state) = states.get_mut(state_index) {
        state.dependencies = node_deps;
    }
    match &node.content {
        Content::Button { label, .. } => {
            if let Some(state) = states.get_mut(state_index) {
                state.text.push_str(label);
            }
        }
        Content::Text { text, .. } => {
            if let Some(state) = states.get_mut(state_index) {
                state.text.reserve(if node.row.is_some() {
                    REPEATED_TEXT_CAPACITY
                } else {
                    text.source().len()
                });
            }
        }
        Content::Action { label, .. }
        | Content::Toggle { label, .. }
        | Content::Slider { label, .. }
        | Content::Choice { label, .. } => {
            if let Some(state) = states.get_mut(state_index) {
                state.text.reserve(if node.row.is_some() {
                    REPEATED_TEXT_CAPACITY
                } else {
                    label.source().len()
                });
            }
        }
        _ => {}
    }
    let mut children = Vec::with_capacity(node.children.len());
    for (child_index, child) in node.children.iter().enumerate() {
        let child_path = format!("{path}/{child_index}");
        children.push(build_tree(child, &child_path, tree, states, deps)?);
    }
    tree.set_children(id, &children).map_err(layout_err)?;
    Ok(id)
}
