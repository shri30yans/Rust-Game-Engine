//! Builders for containers (`body`, `div`), text (`p`, `span`), progress bars and repeat templates.
//! Invariant: a container holds elements only, and a text element holds text only.

use engine_ui::Template;
use engine_ui::doc::{Content, Node};
use markup5ever_rcdom::NodeData;
use std::collections::BTreeMap;

use crate::error::CookError;

use super::convert::convert;
use super::dom::{tag_of, text_of};
use super::element::Element;
use super::props::Props;

/// A `body` or `div`: its element children become the node's children.
pub(super) fn container(el: &Element<'_>, node: &mut Node) -> Result<(), CookError> {
    if el.font_size.is_some() || el.color.is_some() {
        return Err(CookError::UnsupportedProperty {
            element: el.who.to_owned(),
            property: "font-size/color (not inherited; set them on the text)".to_owned(),
        });
    }
    let mut occurrences = BTreeMap::<String, usize>::new();
    for child in el.handle.children.borrow().iter() {
        match &child.data {
            NodeData::Element { .. } => {
                let tag = tag_of(child).unwrap_or_default();
                let attrs = super::dom::attributes(child);
                let base = source_segment(&tag, &attrs);
                let occurrence = occurrences.entry(base.clone()).or_default();
                let child_path = format!("{}>{base}:{}", el.path, *occurrence);
                *occurrence += 1;
                node.children.push(convert(child, el.rules, &child_path)?);
            }
            NodeData::Text { contents } if !contents.borrow().trim().is_empty() => {
                return Err(el.structure("loose text in a container; wrap it in <p>"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn source_segment(tag: &str, attrs: &std::collections::BTreeMap<String, String>) -> String {
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
    format!("{tag}{classes}{id}")
}

/// A `p` or `span`: its whitespace-collapsed text, with the font size and colour it states.
pub(super) fn text(el: &Element<'_>, node: &mut Node) -> Result<(), CookError> {
    if el
        .handle
        .children
        .borrow()
        .iter()
        .any(|c| tag_of(c).is_some())
    {
        return Err(el.structure("a text element holds text only"));
    }
    let text = text_of(el.handle)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    node.content = Content::Text {
        text: Template::parse(&text)?,
        size: el.needs(el.font_size, "font-size")?,
        color: el.needs_colour()?,
        align: el.text_align,
    };
    Ok(())
}

/// A `progress` bar showing the bound value.
pub(super) fn bar(el: &Element<'_>, node: &mut Node) -> Result<(), CookError> {
    let value = el.attr("data-bind")?;
    node.content = Content::Bar {
        value: value.clone(),
        color: el.needs_colour()?,
    };
    Ok(())
}

/// A `template` with `data-repeat`: one element child, stamped once per item up to `data-max`.
pub(super) fn repeat(el: &Element<'_>, p: &Props, node: &mut Node) -> Result<(), CookError> {
    let source = el.attr("data-repeat")?.clone();
    let max = el
        .attr("data-max")?
        .parse::<u16>()
        .map_err(|_| p.bad("data-max", "an integer from 1 to 1024"))?;
    let template_root = match &el.handle.data {
        NodeData::Element {
            template_contents, ..
        } => template_contents
            .borrow()
            .clone()
            .unwrap_or_else(|| el.handle.clone()),
        _ => el.handle.clone(),
    };
    let child = template_root
        .children
        .borrow()
        .iter()
        .find(|child| tag_of(child).is_some())
        .cloned()
        .ok_or_else(|| el.structure("a repeat template has one element child"))?;
    let template = convert(&child, el.rules, &format!("{}/template", el.path))?;
    node.content = Content::Repeat {
        source,
        max,
        template: Box::new(template),
    };
    Ok(())
}
