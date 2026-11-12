//! Validates interactive document widgets and bounded repeat bindings.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::collections::BTreeMap;

use engine_ui::doc::{Color, Content, Len, Node, ScreenDef, StateStyles, StyleDelta, TextAlign};
use engine_ui::{
    BackRule, Bindings, Document, ListBinding, ListRecord, MetricsKey, TextMeasure, UiError,
    UiLayout,
};

struct Measure;
impl TextMeasure for Measure {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(1)
    }
    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * text.len() as f32 * 0.5
    }
}

fn action(id: &str) -> Node {
    Node {
        content: Content::Action {
            id: id.to_owned(),
            label: engine_ui::Template::parse(id).unwrap(),
            disabled: None,
        },
        ..Node::default()
    }
}

fn record(name: &str, score: f32) -> ListRecord {
    let mut row = BTreeMap::new();
    row.insert("name".to_owned(), engine_ui::Value::Text(name.to_owned()));
    row.insert("score".to_owned(), engine_ui::Value::Num(score));
    row.insert("ready".to_owned(), engine_ui::Value::Flag(false));
    row
}

#[test]
fn screen_focus_and_back_rules_must_resolve_to_widgets() {
    let root = Node {
        children: vec![action("play")],
        ..Node::default()
    };
    let screen = ScreenDef {
        default_focus: Some("play".to_owned()),
        back: BackRule::Action("play".to_owned()),
    };
    assert!(Document::with_screen(root.clone(), Some(screen)).is_ok());
    assert!(matches!(
        Document::with_screen(
            root.clone(),
            Some(ScreenDef {
                default_focus: Some("missing".to_owned()),
                ..ScreenDef::default()
            })
        ),
        Err(UiError::UnknownDefaultFocus(_))
    ));
    assert!(matches!(
        Document::with_screen(
            root,
            Some(ScreenDef {
                back: BackRule::Action("missing".to_owned()),
                ..ScreenDef::default()
            })
        ),
        Err(UiError::UnknownBackAction(_))
    ));
}

#[test]
fn widgets_enforce_unique_ids_and_slider_ranges() {
    let duplicate = Node {
        children: vec![action("go"), action("go")],
        ..Node::default()
    };
    assert!(matches!(
        Document::new(duplicate),
        Err(UiError::DocInvalid { .. })
    ));
    let slider = Node {
        content: Content::Slider {
            id: "volume".to_owned(),
            label: engine_ui::Template::parse("Volume").unwrap(),
            value: "volume".to_owned(),
            min: 1.0,
            max: 1.0,
            step: 0.0,
            disabled: None,
        },
        ..Node::default()
    };
    assert!(matches!(
        Document::new(slider),
        Err(UiError::DocInvalid { .. })
    ));
}

#[test]
fn state_styles_are_restricted_to_widgets_and_apply_precedence_data() {
    let mut widget = action("go");
    widget.style.states = StateStyles {
        hover: Some(StyleDelta {
            opacity: Some(0.5),
            ..StyleDelta::default()
        }),
        ..StateStyles::default()
    };
    assert!(Document::new(widget).is_ok());
    let mut plain = Node::default();
    plain.style.states = StateStyles {
        hover: Some(StyleDelta::default()),
        ..StateStyles::default()
    };
    assert!(matches!(
        Document::new(plain),
        Err(UiError::DocInvalid { .. })
    ));
}

#[test]
fn repeat_grows_and_shrinks_within_capacity_and_rejects_overflow() {
    let template = Node {
        layout: engine_ui::doc::LayoutDef {
            display: engine_ui::doc::Display::Flex,
            width: Len::Px(300.0),
            height: Len::Px(50.0),
            direction: engine_ui::doc::Direction::Row,
            ..engine_ui::doc::LayoutDef::default()
        },
        style: engine_ui::doc::StyleDef {
            fill: Some(Color::parse("#123456").unwrap()),
            ..engine_ui::doc::StyleDef::default()
        },
        children: vec![
            Node {
                layout: engine_ui::doc::LayoutDef {
                    display: engine_ui::doc::Display::Flex,
                    width: Len::Px(100.0),
                    height: Len::Px(50.0),
                    ..engine_ui::doc::LayoutDef::default()
                },
                content: Content::Text {
                    text: engine_ui::Template::parse("{item.name}").unwrap(),
                    size: 18.0,
                    color: Color::parse("#ffffff").unwrap(),
                    align: TextAlign::Start,
                },
                ..Node::default()
            },
            Node {
                layout: engine_ui::doc::LayoutDef {
                    display: engine_ui::doc::Display::Flex,
                    width: Len::Px(100.0),
                    height: Len::Px(50.0),
                    ..engine_ui::doc::LayoutDef::default()
                },
                content: Content::Text {
                    text: engine_ui::Template::parse("{item.score:0}").unwrap(),
                    size: 18.0,
                    color: Color::parse("#ffffff").unwrap(),
                    align: TextAlign::Start,
                },
                ..Node::default()
            },
            Node {
                layout: engine_ui::doc::LayoutDef {
                    display: engine_ui::doc::Display::Flex,
                    width: Len::Px(100.0),
                    height: Len::Px(50.0),
                    ..engine_ui::doc::LayoutDef::default()
                },
                content: Content::Action {
                    id: "kick".to_owned(),
                    label: engine_ui::Template::parse("Kick").unwrap(),
                    disabled: Some("item.ready".to_owned()),
                },
                ..Node::default()
            },
        ],
        ..Node::default()
    };
    let repeat = Node {
        layout: engine_ui::doc::LayoutDef {
            display: engine_ui::doc::Display::Flex,
            width: Len::Px(300.0),
            height: Len::Px(150.0),
            direction: engine_ui::doc::Direction::Column,
            ..engine_ui::doc::LayoutDef::default()
        },
        content: Content::Repeat {
            source: "items".to_owned(),
            max: 3,
            template: Box::new(template),
        },
        ..Node::default()
    };
    let mut layout = UiLayout::new(Document::new(repeat).unwrap()).unwrap();
    let mut bindings = Bindings::new();
    let mut list = ListBinding::with_capacity(3);
    list.replace(&[record("Alpha", 4.0)]);
    bindings.set_list("items", list.clone());
    let mut measure = Measure;
    let draw = layout
        .frame(&bindings, (400.0, 300.0), &mut measure)
        .unwrap();
    assert_eq!(draw.widgets().len(), 1);
    list.replace(&[
        record("Alpha", 4.0),
        record("Bravo", 8.0),
        record("Charlie", 12.0),
    ]);
    bindings.set_list("items", list.clone());
    let draw = layout
        .frame(&bindings, (400.0, 300.0), &mut measure)
        .unwrap();
    let rows: Vec<_> = draw
        .primitives()
        .iter()
        .filter_map(|primitive| match primitive {
            engine_ui::Primitive::Text { range, .. } => draw.text_of(range),
            _ => None,
        })
        .collect();
    assert_eq!(
        rows,
        [
            "Alpha", "4", "Kick", "Bravo", "8", "Kick", "Charlie", "12", "Kick"
        ]
    );
    assert_eq!(draw.widgets().len(), 3);
    assert_eq!(
        draw.widgets().get(2).map(|widget| widget.row),
        Some(Some(engine_ui::RowIndex(2)))
    );
    list.replace(&[record("Bravo", 8.0)]);
    bindings.set_list("items", list);
    assert_eq!(
        layout
            .frame(&bindings, (400.0, 300.0), &mut measure)
            .unwrap()
            .widgets()
            .len(),
        1
    );
    let mut too_many = ListBinding::with_capacity(4);
    too_many.replace(&[
        record("1", 1.0),
        record("2", 2.0),
        record("3", 3.0),
        record("4", 4.0),
    ]);
    bindings.set_list("items", too_many);
    assert!(matches!(
        layout.frame(&bindings, (400.0, 300.0), &mut measure),
        Err(UiError::RepeatTooLong { .. })
    ));
}
