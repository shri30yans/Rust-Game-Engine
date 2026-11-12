//! Golden draw lists for a home screen at desktop and phone dimensions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use engine_ui::doc::{Align, Color, Content, Direction, Justify, LayoutDef, Len, Node, TextAlign};
use engine_ui::{
    Bindings, Document, ListBinding, ListRecord, MetricsKey, TextMeasure, UiLayout, Value,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;
use std::path::PathBuf;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

struct Measure;

impl TextMeasure for Measure {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(4)
    }

    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * text.chars().count() as f32 * 0.5
    }
}

fn list_record(entries: &[(&str, Value)]) -> ListRecord {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn list(records: Vec<ListRecord>) -> ListBinding {
    let mut list = ListBinding::with_capacity(records.len());
    list.replace(&records);
    list
}

fn node(
    layout: LayoutDef,
    style: engine_ui::doc::StyleDef,
    content: Content,
    children: Vec<Node>,
) -> Node {
    Node {
        layout,
        style,
        content,
        children,
        ..Node::default()
    }
}

fn fixed(width: Len, height: Len, direction: Direction) -> LayoutDef {
    LayoutDef {
        display: engine_ui::doc::Display::Flex,
        width,
        height,
        direction,
        gap: 12.0,
        padding: 12.0,
        align: Align::Stretch,
        justify: Justify::Start,
        ..LayoutDef::default()
    }
}

fn text(content: &str, width: f32, height: f32) -> Node {
    node(
        fixed(Len::Px(width), Len::Px(height), Direction::Row),
        engine_ui::doc::StyleDef::default(),
        Content::Text {
            text: engine_ui::Template::parse(content).unwrap(),
            size: 20.0,
            color: Color::parse("#f4f6fa").unwrap(),
            align: TextAlign::Start,
        },
        Vec::new(),
    )
}

fn home_document() -> Document {
    let card = node(
        fixed(Len::Px(280.0), Len::Px(150.0), Direction::Column),
        engine_ui::doc::StyleDef {
            fill: Some(Color::parse("#1f2937").unwrap()),
            radius: 12.0,
            ..engine_ui::doc::StyleDef::default()
        },
        Content::None,
        vec![
            node(
                fixed(Len::Px(256.0), Len::Px(72.0), Direction::Column),
                engine_ui::doc::StyleDef {
                    fill: Some(Color::parse("#35465f").unwrap()),
                    ..engine_ui::doc::StyleDef::default()
                },
                Content::None,
                vec![
                    text("{item.title}", 240.0, 28.0),
                    text("{item.badge}", 240.0, 24.0),
                ],
            ),
            node(
                fixed(Len::Px(120.0), Len::Px(40.0), Direction::Row),
                engine_ui::doc::StyleDef::default(),
                Content::Action {
                    id: "open_card".to_owned(),
                    label: engine_ui::Template::parse("Open").unwrap(),
                    disabled: None,
                },
                Vec::new(),
            ),
        ],
    );
    let cards = node(
        fixed(Len::Percent(1.0), Len::Px(174.0), Direction::Row),
        engine_ui::doc::StyleDef::default(),
        Content::Repeat {
            source: "cards".to_owned(),
            max: 3,
            template: Box::new(card),
        },
        Vec::new(),
    );
    let tabs = node(
        fixed(Len::Percent(1.0), Len::Px(48.0), Direction::Row),
        engine_ui::doc::StyleDef {
            text_color: Some(Color::parse("#ffffff").unwrap()),
            ..engine_ui::doc::StyleDef::default()
        },
        Content::Tabs {
            id: "home_tabs".to_owned(),
            tabs: "tabs".to_owned(),
            selected: "tab_index".to_owned(),
            disabled: None,
        },
        Vec::new(),
    );
    let settings = node(
        fixed(Len::Percent(1.0), Len::Px(220.0), Direction::Column),
        engine_ui::doc::StyleDef {
            fill: Some(Color::parse("#111827").unwrap()),
            radius: 8.0,
            ..engine_ui::doc::StyleDef::default()
        },
        Content::None,
        vec![
            node(
                fixed(Len::Percent(1.0), Len::Px(40.0), Direction::Row),
                engine_ui::doc::StyleDef::default(),
                Content::Toggle {
                    id: "music".to_owned(),
                    label: engine_ui::Template::parse("Music").unwrap(),
                    value: "music".to_owned(),
                    disabled: None,
                },
                Vec::new(),
            ),
            node(
                fixed(Len::Percent(1.0), Len::Px(40.0), Direction::Row),
                engine_ui::doc::StyleDef::default(),
                Content::Slider {
                    id: "volume".to_owned(),
                    label: engine_ui::Template::parse("Volume").unwrap(),
                    value: "volume".to_owned(),
                    min: 0.0,
                    max: 1.0,
                    step: 0.05,
                    disabled: None,
                },
                Vec::new(),
            ),
            node(
                fixed(Len::Percent(1.0), Len::Px(40.0), Direction::Row),
                engine_ui::doc::StyleDef::default(),
                Content::Choice {
                    id: "quality".to_owned(),
                    label: engine_ui::Template::parse("Quality").unwrap(),
                    options: "qualities".to_owned(),
                    selected: "quality_index".to_owned(),
                    disabled: None,
                },
                Vec::new(),
            ),
        ],
    );
    let root = node(
        fixed(Len::Percent(1.0), Len::Percent(1.0), Direction::Column),
        engine_ui::doc::StyleDef {
            fill: Some(Color::parse("#0b1220").unwrap()),
            ..engine_ui::doc::StyleDef::default()
        },
        Content::None,
        vec![tabs, cards, settings],
    );
    Document::new(root).unwrap()
}

fn bindings() -> Bindings {
    let mut bindings = Bindings::new();
    bindings.set_list(
        "tabs",
        list(vec![
            list_record(&[("label", Value::Text("Play".to_owned()))]),
            list_record(&[("label", Value::Text("Modes".to_owned()))]),
            list_record(&[("label", Value::Text("Settings".to_owned()))]),
        ]),
    );
    bindings.set_num("tab_index", 0.0);
    bindings.set_list(
        "cards",
        list(vec![
            list_record(&[
                ("title", Value::Text("Quick Match".to_owned())),
                ("badge", Value::Text("3 seats".to_owned())),
            ]),
            list_record(&[
                ("title", Value::Text("Practice".to_owned())),
                ("badge", Value::Text("Offline".to_owned())),
            ]),
        ]),
    );
    bindings.set_flag("music", true);
    bindings.set_num("volume", 0.65);
    bindings.set_list(
        "qualities",
        list(vec![
            list_record(&[("label", Value::Text("Low".to_owned()))]),
            list_record(&[("label", Value::Text("High".to_owned()))]),
        ]),
    );
    bindings.set_num("quality_index", 1.0);
    bindings
}

fn snapshot(screen_size: (f32, f32)) -> String {
    let mut layout = UiLayout::new(home_document()).unwrap();
    let bindings = bindings();
    layout.frame(&bindings, screen_size, &mut Measure).unwrap();
    let region = Region::new(GLOBAL);
    let draw = layout.frame(&bindings, screen_size, &mut Measure).unwrap();
    let stats = region.change();
    assert_eq!(
        stats.allocations, 0,
        "steady golden frame allocated: {stats:?}"
    );
    assert_eq!(
        stats.reallocations, 0,
        "steady golden frame reallocated: {stats:?}"
    );
    format!("{draw:#?}\nTEXT: {:?}\n", draw.text())
}

fn check_golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    if std::env::var_os("UPDATE_MENU_GOLDENS").is_some() {
        std::fs::write(path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(actual, expected, "golden changed: {}", path.display());
}

#[test]
fn home_draw_list_matches_desktop_and_phone_goldens() {
    check_golden("home-menu-desktop.golden", &snapshot((1920.0, 1080.0)));
    check_golden("home-menu-phone.golden", &snapshot((844.0, 390.0)));
}
