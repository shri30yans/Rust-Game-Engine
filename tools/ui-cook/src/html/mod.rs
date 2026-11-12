//! Turns a parsed page into a UI document.
//!
//! Elements: `body` and `div` are containers; `p` and `span` are text; `progress` is a bar;
//! `button` is a touch button. Attributes: `id`, `class`, `style`, `data-show-if`, `data-bind`,
//! `data-id`, `data-held`, `data-drag-look` (a button's finger also aims). Styles come from one
//! `<style>` block and `style` attributes, with the usual order: tag, then class, then id, then
//! the inline style, later wins on a tie.
//!
//! Invariants: nothing outside the subset is dropped; it is an error. Style is not inherited, so
//! text, bars and buttons state their own `font-size` and `color`.
//!
//! Layout of this folder: `dom` walks the parsed page, `cascade` picks each element's declarations,
//! `props` reads typed values from them, `look` and `state` build layout and style, `convert`
//! builds one node, and `containers` and `widgets` build the element content.

mod cascade;
mod containers;
mod convert;
mod dom;
mod element;
mod look;
mod props;
mod state;
mod widgets;

use engine_ui::doc::{BackRule, Node, ScreenDef};
use html5ever::tendril::TendrilSink;
use html5ever::{ParseOpts, parse_document};
use markup5ever_rcdom::RcDom;

use crate::css::parse_stylesheet;
use crate::error::CookError;

/// Compiles `html` to the root node of a document.
///
/// # Errors
///
/// Returns a [`CookError`] naming the first construct outside the subset.
pub fn cook_root(html: &str) -> Result<(Node, Option<ScreenDef>), CookError> {
    let dom = parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .map_err(|e| CookError::Html(e.to_string()))?;
    let mut css = include_str!("../../ua.css").to_owned();
    let mut body = None;
    dom::find(&dom.document, &mut css, &mut body);
    let body = body.ok_or(CookError::NoBody)?;
    let rules = parse_stylesheet(&css)?;
    let attrs = dom::attributes(&body);
    let screen = if attrs.contains_key("data-default-focus") || attrs.contains_key("data-back") {
        Some(ScreenDef {
            default_focus: attrs.get("data-default-focus").cloned(),
            back: attrs
                .get("data-back")
                .cloned()
                .map_or(BackRule::Pop, BackRule::Action),
        })
    } else {
        None
    };
    Ok((convert::convert(&body, &rules, "body")?, screen))
}
