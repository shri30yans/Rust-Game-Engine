//! The DOM walk: finds the `<style>` text and the `<body>`, and reads tags, text and attributes.
//! Invariant: `find` stops descending at `<body>`, so only the page's own styles are collected.

use std::collections::BTreeMap;

use markup5ever_rcdom::{Handle, NodeData};

/// The element name, or `None` for text and comments.
pub(super) fn tag_of(handle: &Handle) -> Option<String> {
    match &handle.data {
        NodeData::Element { name, .. } => Some(name.local.to_string()),
        _ => None,
    }
}

/// Collects every `<style>` text into `css` and records the `<body>`.
pub(super) fn find(handle: &Handle, css: &mut String, body: &mut Option<Handle>) {
    match tag_of(handle).as_deref() {
        Some("style") => css.push_str(&text_of(handle)),
        Some("body") => *body = Some(handle.clone()),
        _ => {}
    }
    if tag_of(handle).as_deref() != Some("body") {
        for child in handle.children.borrow().iter() {
            find(child, css, body);
        }
    }
}

/// The text children of an element, concatenated.
pub(super) fn text_of(handle: &Handle) -> String {
    let mut out = String::new();
    for child in handle.children.borrow().iter() {
        if let NodeData::Text { contents } = &child.data {
            out.push_str(&contents.borrow());
        }
    }
    out
}

/// The element's attributes by name.
pub(super) fn attributes(handle: &Handle) -> BTreeMap<String, String> {
    match &handle.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .map(|a| (a.name.local.to_string(), a.value.to_string()))
            .collect(),
        _ => BTreeMap::new(),
    }
}
