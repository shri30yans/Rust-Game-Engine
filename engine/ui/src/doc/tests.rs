//! Document validation and serialization tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use crate::error::UiError;

const MINIMAL: &str = r##"(version: 1, root: ())"##;

#[test]
fn a_minimal_document_loads() -> Result<(), UiError> {
    let doc = Document::from_ron(MINIMAL)?;
    assert_eq!(doc.root(), &Node::default());
    Ok(())
}

#[test]
fn an_unspecified_layout_uses_css_block_display_and_row_direction() {
    let layout = LayoutDef::default();
    assert_eq!(layout.display, Display::Block);
    assert_eq!(layout.direction, Direction::Row);
}

fn err(source: &str) -> UiError {
    match Document::from_ron(source) {
        Ok(_) => UiError::EmptyStack,
        Err(e) => e,
    }
}

#[test]
fn a_wrong_version_is_rejected() {
    assert!(matches!(
        err("(version: 2, root: ())"),
        UiError::DocInvalid { .. }
    ));
}

#[test]
fn an_unknown_field_is_rejected() {
    assert!(matches!(
        err("(version: 1, root: (colour: 1))"),
        UiError::DocParse(_)
    ));
}

#[test]
fn colours_parse_with_and_without_alpha() -> Result<(), UiError> {
    assert_eq!(Color::parse("#ff0000")?.a, 255);
    assert_eq!(Color::parse("#ff000080")?.a, 128);
    assert!(Color::parse("red").is_err());
    assert!(Color::parse("#ff00").is_err());
    assert!(Color::parse("#ff0000zz").is_err());
    Ok(())
}

#[test]
fn out_of_range_numbers_are_rejected() {
    for body in [
        "layout: (width: Px(-1.0))",
        "layout: (width: Percent(1.5))",
        "layout: (gap: -2.0)",
        "layout: (grow: -1.0)",
        "style: (opacity: 2.0)",
        "style: (radius: -1.0)",
    ] {
        let source = format!("(version: 1, root: ({body}))");
        assert!(matches!(err(&source), UiError::DocInvalid { .. }), "{body}");
    }
}

#[test]
fn duplicate_ids_and_button_ids_are_rejected() {
    let ids = r##"(version: 1, root: (children: [(id: Some("a")), (id: Some("a"))]))"##;
    assert!(matches!(err(ids), UiError::DocInvalid { .. }));
    let button = r##"(version: 1, root: (children: [
            (layout: (position: Absolute(left: Some(Px(0.0)))), content: Button(id: 1, label: "A", color: "#fff", held: "a")),
            (layout: (position: Absolute(left: Some(Px(0.0)))), content: Button(id: 1, label: "B", color: "#fff", held: "b"))]))"##;
    assert!(matches!(
        err(button),
        UiError::DocInvalid { .. } | UiError::DocParse(_)
    ));
}

#[test]
fn a_text_node_cannot_have_children() {
    let source = r##"(version: 1, root: (content: Text(text: "x", size: 10.0, color: "#ffffff"), children: [()]))"##;
    assert!(matches!(err(source), UiError::DocInvalid { .. }));
}

#[test]
fn a_bad_template_is_a_parse_error() {
    let source =
        r##"(version: 1, root: (content: Text(text: "{", size: 10.0, color: "#ffffff")))"##;
    assert!(matches!(err(source), UiError::DocParse(_)));
}

#[test]
fn the_error_names_the_node_path() {
    let source = "(version: 1, root: (children: [(), (layout: (gap: -1.0))]))";
    assert!(matches!(err(source), UiError::DocInvalid { node, .. } if node == "root/1"));
}
