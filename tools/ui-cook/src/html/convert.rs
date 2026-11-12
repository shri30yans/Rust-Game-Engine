//! Converts one element into a node: checks its attributes, reads its style, then builds its
//! content. Invariant: an attribute, property or element outside the subset is an error, never
//! silently dropped.

use std::collections::BTreeMap;

use engine_ui::doc::{Len, Node};
use markup5ever_rcdom::Handle;
use markup5ever_rcdom::NodeData;

use crate::css::Rule;
use crate::error::CookError;

use super::cascade::cascade;
use super::containers;
use super::dom::{attributes, tag_of};
use super::element::Element;
use super::look::look;
use super::props::Props;
use super::widgets;

/// Attributes the cooker understands. Any other attribute is an error.
const ATTRIBUTES: &[&str] = &[
    "id",
    "class",
    "style",
    "data-show-if",
    "data-bind",
    "data-id",
    "data-held",
    "data-drag-look",
    "data-action",
    "data-options",
    "data-max",
    "data-repeat",
    "data-default-focus",
    "data-back",
    "type",
    "min",
    "max",
    "step",
    "role",
    "aria-label",
    "data-disabled",
];

/// Converts the element `handle` at `path` into a node and its children.
pub(super) fn convert(handle: &Handle, rules: &[Rule], path: &str) -> Result<Node, CookError> {
    let tag = tag_of(handle).unwrap_or_default();
    let attrs = attributes(handle);
    let who = describe(&tag, &attrs, path);
    let has_elements = handle
        .children
        .borrow()
        .iter()
        .any(|child| matches!(child.data, NodeData::Element { .. }));
    let has_text = handle.children.borrow().iter().any(|child| {
        matches!(&child.data, NodeData::Text { contents } if !contents.borrow().trim().is_empty())
    });
    if has_elements && has_text {
        return Err(CookError::Structure {
            element: who,
            reason: "an element holds child elements or one text run, never both",
        });
    }
    for name in attrs.keys() {
        if !ATTRIBUTES.contains(&name.as_str()) {
            return Err(CookError::UnsupportedAttribute {
                element: who,
                attribute: name.clone(),
            });
        }
    }
    if tag != "body"
        && (attrs.contains_key("data-default-focus") || attrs.contains_key("data-back"))
    {
        return Err(CookError::Structure {
            element: who.clone(),
            reason: "screen navigation attributes belong on <body>",
        });
    }
    let mut p = Props {
        props: cascade(rules, &tag, &attrs)?,
        who: who.clone(),
    };
    let look = look(&mut p)?;
    if matches!(tag.as_str(), "input" | "select")
        && (matches!(look.layout.width, Len::Auto) || matches!(look.layout.height, Len::Auto))
    {
        return Err(CookError::MissingProperty {
            element: who.clone(),
            property: if matches!(look.layout.width, Len::Auto) {
                "width"
            } else {
                "height"
            },
        });
    }
    let mut node = Node {
        source: Some(if path == "body" {
            source_path(&tag, &attrs, path)
        } else {
            path.to_owned()
        }),
        id: attrs.get("id").cloned(),
        layout: look.layout,
        style: look.style,
        fit: look.fit,
        text_overflow: look.text_overflow,
        show_if: attrs.get("data-show-if").cloned(),
        ..Node::default()
    };
    let el = Element {
        handle,
        rules,
        path,
        tag: &tag,
        attrs: &attrs,
        who: &who,
        font_size: look.font_size,
        color: look.color,
        text_align: look.text_align,
    };
    build(&el, &p, &mut node)?;
    if let Some(name) = p.props.keys().next() {
        return Err(CookError::UnsupportedProperty {
            element: who.clone(),
            property: name.clone(),
        });
    }
    Ok(node)
}

/// Sets the node's content from its element name.
fn build(el: &Element<'_>, p: &Props, node: &mut Node) -> Result<(), CookError> {
    match el.tag {
        "div" if el.attrs.get("role").is_some_and(|role| role == "tablist") => {
            widgets::tabs(el, node)
        }
        "body" | "div" => containers::container(el, node),
        "p" | "span" => containers::text(el, node),
        "progress" => containers::bar(el, node),
        "template" => containers::repeat(el, p, node),
        "button" => widgets::button(el, p, node),
        "input" => widgets::input(el, p, node),
        "select" => widgets::select(el, node),
        other => Err(CookError::UnsupportedElement(other.to_owned())),
    }
}

/// The element as shown in errors: its tag, its id if it has one, and its place in the page.
fn describe(tag: &str, attrs: &BTreeMap<String, String>, path: &str) -> String {
    match attrs.get("id") {
        Some(id) => format!("<{tag} id=\"{id}\"> ({path})"),
        None => format!("<{tag}> ({path})"),
    }
}

/// Builds the stable CSS-like path stored with every cooked node.
fn source_path(tag: &str, attrs: &BTreeMap<String, String>, parent: &str) -> String {
    if tag == "body" {
        return "body".to_owned();
    }
    let classes = attrs
        .get("class")
        .map(|value| {
            value
                .split_whitespace()
                .map(|class| format!(".{class}"))
                .collect::<String>()
        })
        .unwrap_or_default();
    let id = attrs
        .get("id")
        .map(|value| format!("#{value}"))
        .unwrap_or_default();
    if parent.is_empty() {
        format!("{tag}{classes}{id}")
    } else {
        format!("{parent}>{tag}{classes}{id}")
    }
}
