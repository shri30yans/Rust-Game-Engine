//! Proves steady menu routing, focus, hover and visual-state updates allocate nothing.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_ui::doc::{Color, Content, Display, Len, Node, StateStyles, StyleDelta};
use engine_ui::{
    Bindings, Document, Layer, ListBinding, ListRecord, MenuInput, MenuMotion, Menus, MotionEntry,
    NavInput, Value,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::alloc::System;
use std::collections::BTreeMap;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn widget(id: &str) -> Node {
    Node {
        layout: engine_ui::doc::LayoutDef {
            width: Len::Px(120.0),
            height: Len::Px(40.0),
            ..engine_ui::doc::LayoutDef::default()
        },
        style: engine_ui::doc::StyleDef {
            states: StateStyles {
                focus: Some(StyleDelta {
                    opacity: Some(0.9),
                    ..StyleDelta::default()
                }),
                hover: Some(StyleDelta {
                    fill: Some(Color::parse("#123456").unwrap()),
                    ..StyleDelta::default()
                }),
                ..StateStyles::default()
            },
            ..engine_ui::doc::StyleDef::default()
        },
        content: Content::Action {
            id: id.to_owned(),
            label: engine_ui::Template::parse(id).unwrap(),
            disabled: None,
        },
        ..Node::default()
    }
}

fn row(name: &str) -> ListRecord {
    let mut record = BTreeMap::new();
    record.insert("name".to_owned(), Value::Text(name.to_owned()));
    record
}

fn repeat_document() -> Document {
    let text = |source: &str| Node {
        layout: engine_ui::doc::LayoutDef {
            width: Len::Px(120.0),
            height: Len::Px(24.0),
            ..engine_ui::doc::LayoutDef::default()
        },
        content: Content::Text {
            text: engine_ui::Template::parse(source).unwrap(),
            size: 18.0,
            color: Color::parse("#ffffff").unwrap(),
            align: engine_ui::doc::TextAlign::Start,
        },
        ..Node::default()
    };
    let template = Node {
        layout: engine_ui::doc::LayoutDef {
            display: Display::Flex,
            width: Len::Px(300.0),
            height: Len::Px(40.0),
            direction: engine_ui::doc::Direction::Row,
            ..engine_ui::doc::LayoutDef::default()
        },
        children: vec![text("{item.name}"), widget("kick")],
        ..Node::default()
    };
    let repeat = Node {
        layout: engine_ui::doc::LayoutDef {
            display: Display::Flex,
            width: Len::Px(300.0),
            height: Len::Px(120.0),
            direction: engine_ui::doc::Direction::Column,
            ..engine_ui::doc::LayoutDef::default()
        },
        content: Content::Repeat {
            source: "seats".to_owned(),
            max: 3,
            template: Box::new(template),
        },
        ..Node::default()
    };
    Document::new(repeat).unwrap()
}

#[test]
fn one_thousand_menu_frames_allocate_nothing_after_warm_up() {
    let root = Node {
        children: vec![widget("play"), widget("settings")],
        ..Node::default()
    };
    let document = Document::new(root).unwrap();
    let motion = MenuMotion {
        screen: MotionEntry {
            ms: 220,
            curve: (0.4, 0.0, 0.2, 1.0),
        },
        modal: MotionEntry {
            ms: 180,
            curve: (0.2, 0.0, 0.0, 1.0),
        },
        exit_ms: 120,
        slide_px: 24.0,
    };
    let mut menus = Menus::new(motion);
    menus.push(Layer::Menu, document).unwrap();
    let bindings = Bindings::new();
    let mut input = MenuInput::new();
    menus.frame(&input, &bindings, (800.0, 600.0), 300).unwrap();
    for frame in 0..1_000 {
        input.nav = Some(if frame % 2 == 0 {
            NavInput::Next
        } else {
            NavInput::Prev
        });
        input.pointer.position = if frame % 2 == 0 {
            glam::Vec2::new(50.0, 20.0)
        } else {
            glam::Vec2::new(150.0, 20.0)
        };
        input.pointer.pressed = frame % 10 == 0;
        input.confirm = frame % 20 == 0;
        let region = Region::new(GLOBAL);
        menus
            .frame(&input, &bindings, (800.0, 600.0), 301 + frame)
            .unwrap();
        let stats = region.change();
        assert_eq!(stats.allocations, 0, "frame {frame} allocated: {stats:?}");
        assert_eq!(
            stats.reallocations, 0,
            "frame {frame} reallocated: {stats:?}"
        );
    }
    changing_repeat_length_frames_allocate_nothing_after_warm_up();
}

fn changing_repeat_length_frames_allocate_nothing_after_warm_up() {
    let mut short = ListBinding::with_capacity(3);
    short.replace(&[row("One")]);
    let mut full = ListBinding::with_capacity(3);
    full.replace(&[row("One"), row("Two"), row("Three")]);
    let mut bindings = Bindings::new();
    bindings.set_list("seats", short.clone());
    let mut menus = Menus::new(MenuMotion {
        screen: MotionEntry {
            ms: 220,
            curve: (0.4, 0.0, 0.2, 1.0),
        },
        modal: MotionEntry {
            ms: 180,
            curve: (0.2, 0.0, 0.0, 1.0),
        },
        exit_ms: 120,
        slide_px: 24.0,
    });
    menus.push(Layer::Menu, repeat_document()).unwrap();
    let input = MenuInput::new();
    menus.frame(&input, &bindings, (800.0, 600.0), 300).unwrap();

    for frame in 0..1_000 {
        let rows = if frame % 2 == 0 {
            full.clone()
        } else {
            short.clone()
        };
        bindings.set_list("seats", rows);
        let region = Region::new(GLOBAL);
        menus
            .frame(&input, &bindings, (800.0, 600.0), 301 + frame)
            .unwrap();
        let stats = region.change();
        assert_eq!(stats.allocations, 0, "menu frame allocated: {stats:?}");
        assert_eq!(stats.reallocations, 0, "menu frame reallocated: {stats:?}");
    }
}
