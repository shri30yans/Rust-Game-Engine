//! Exercises menu input routing, focus, Back behaviour and exit retention.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use engine_ui::doc::Node;
use engine_ui::{
    BackRule, Bindings, Document, Layer, ListBinding, ListRecord, MenuInput, MenuMotion, Menus,
    MotionEntry, NavInput, RowIndex, UiEvent, Value,
};
use std::collections::BTreeMap;

fn motion() -> MenuMotion {
    MenuMotion {
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
    }
}

fn document(back: Option<&str>) -> Document {
    let source = format!(
        r##"(version:1, screen:Some((default_focus:Some("play"),back:{})), root:(layout:(display:Flex),children:[
      (layout:(display:Flex,width:Px(100.0),height:Px(40.0)),content:Action(id:"play",label:"PLAY")),
      (layout:(display:Flex,width:Px(100.0),height:Px(40.0)),content:Action(id:"settings",label:"SETTINGS"))]))"##,
        back.map_or("Pop".to_owned(), |id| format!("Action(\"{id}\")"))
    );
    Document::from_ron(&source).unwrap()
}

fn repeated_document() -> Document {
    let action = |id: &str, label: &str| Node {
        layout: engine_ui::doc::LayoutDef {
            display: engine_ui::doc::Display::Flex,
            width: engine_ui::doc::Len::Px(120.0),
            height: engine_ui::doc::Len::Px(32.0),
            ..engine_ui::doc::LayoutDef::default()
        },
        content: engine_ui::doc::Content::Action {
            id: id.to_owned(),
            label: engine_ui::Template::parse(label).unwrap(),
            disabled: None,
        },
        ..Node::default()
    };
    let row = Node {
        layout: engine_ui::doc::LayoutDef {
            display: engine_ui::doc::Display::Flex,
            width: engine_ui::doc::Len::Px(300.0),
            height: engine_ui::doc::Len::Px(40.0),
            ..engine_ui::doc::LayoutDef::default()
        },
        children: vec![
            Node {
                layout: engine_ui::doc::LayoutDef {
                    display: engine_ui::doc::Display::Flex,
                    width: engine_ui::doc::Len::Px(150.0),
                    height: engine_ui::doc::Len::Px(32.0),
                    ..engine_ui::doc::LayoutDef::default()
                },
                content: engine_ui::doc::Content::Text {
                    text: engine_ui::Template::parse("{item.name}").unwrap(),
                    size: 18.0,
                    color: engine_ui::doc::Color::parse("#ffffff").unwrap(),
                    align: engine_ui::doc::TextAlign::Start,
                },
                ..Node::default()
            },
            action("kick", "Kick"),
        ],
        ..Node::default()
    };
    let repeat = Node {
        layout: engine_ui::doc::LayoutDef {
            display: engine_ui::doc::Display::Flex,
            width: engine_ui::doc::Len::Px(300.0),
            height: engine_ui::doc::Len::Px(120.0),
            direction: engine_ui::doc::Direction::Column,
            ..engine_ui::doc::LayoutDef::default()
        },
        content: engine_ui::doc::Content::Repeat {
            source: "seats".to_owned(),
            max: 3,
            template: Box::new(row),
        },
        ..Node::default()
    };
    Document::with_screen(
        Node {
            layout: engine_ui::doc::LayoutDef {
                display: engine_ui::doc::Display::Flex,
                direction: engine_ui::doc::Direction::Column,
                ..engine_ui::doc::LayoutDef::default()
            },
            children: vec![repeat, action("cancel", "Cancel")],
            ..Node::default()
        },
        Some(engine_ui::ScreenDef {
            default_focus: Some("cancel".to_owned()),
            ..engine_ui::ScreenDef::default()
        }),
    )
    .unwrap()
}

fn seats(names: &[&str]) -> ListBinding {
    let mut list = ListBinding::with_capacity(3);
    let records: Vec<ListRecord> = names
        .iter()
        .map(|name| {
            let mut record = BTreeMap::new();
            record.insert("name".to_owned(), Value::Text((*name).to_owned()));
            record
        })
        .collect();
    list.replace(&records);
    list
}

#[test]
fn only_top_focusable_screen_receives_input_and_back_action_is_resolved() {
    let mut menus = Menus::new(motion());
    let menu = menus.push(Layer::Menu, document(Some("play"))).unwrap();
    let play = menus.widget_id(menu, "play").unwrap();
    let _toast = menus.push(Layer::Toast, document(None)).unwrap();
    let mut input = MenuInput::new();
    input.confirm = true;
    let events = menus
        .frame(&input, &Bindings::new(), (800.0, 600.0), 0)
        .unwrap();
    assert_eq!(events, [UiEvent::Action(play, None)]);
    input.confirm = false;
    input.back = true;
    let events = menus
        .frame(&input, &Bindings::new(), (800.0, 600.0), 1)
        .unwrap();
    assert_eq!(events, [UiEvent::Action(play, None)]);
}

#[test]
fn back_pops_active_screen_and_retains_it_until_exit_finishes() {
    let mut menus = Menus::new(motion());
    let screen = menus.push(Layer::Menu, document(None)).unwrap();
    let bindings = Bindings::new();
    let mut input = MenuInput::new();
    menus.frame(&input, &bindings, (800.0, 600.0), 0).unwrap();
    input.back = true;
    assert_eq!(
        menus.frame(&input, &bindings, (800.0, 600.0), 1).unwrap(),
        [UiEvent::Back(screen)]
    );
    assert_eq!(menus.draw_list().count(), 1);
    input.back = false;
    menus.frame(&input, &bindings, (800.0, 600.0), 121).unwrap();
    assert_eq!(menus.draw_list().count(), 0);
}

#[test]
fn navigation_moves_focus_between_document_widgets() {
    let mut menus = Menus::new(motion());
    let screen = menus.push(Layer::Menu, document(None)).unwrap();
    let settings = menus.widget_id(screen, "settings").unwrap();
    let mut input = MenuInput::new();
    let bindings = Bindings::new();
    menus.frame(&input, &bindings, (800.0, 600.0), 0).unwrap();
    input.nav = Some(NavInput::Next);
    menus.frame(&input, &bindings, (800.0, 600.0), 16).unwrap();
    input.nav = None;
    input.confirm = true;
    assert_eq!(
        menus.frame(&input, &bindings, (800.0, 600.0), 17).unwrap(),
        [UiEvent::Action(settings, None)]
    );
}

#[test]
fn pointer_hit_testing_uses_the_same_skew_as_drawing() {
    let document = Document::from_ron(
        r#"(version:1,root:(layout:(display:Flex),style:(skew:20.0),children:[
            (layout:(display:Flex,width:Px(140.0),height:Px(50.0)),content:Action(id:"play",label:"PLAY"))]))"#,
    )
    .unwrap();
    let mut menus = Menus::new(motion());
    let screen = menus.push(Layer::Menu, document).unwrap();
    let play = menus.widget_id(screen, "play").unwrap();
    let mut input = MenuInput::new();
    let bindings = Bindings::new();
    menus.frame(&input, &bindings, (800.0, 600.0), 0).unwrap();
    let region = &menus.draw_list().next().unwrap().list.widgets()[0];
    let centre_y = region.rect.y + region.rect.h * 0.5;
    let centre_x = region.rect.x
        + region.rect.w * 0.5
        + region
            .skew
            .map_or(0.0, |skew| skew.lean * (centre_y - skew.pivot_y));
    input.pointer.position = glam::Vec2::new(centre_x, centre_y);
    input.pointer.pressed = true;
    assert_eq!(
        menus.frame(&input, &bindings, (800.0, 600.0), 1).unwrap(),
        [UiEvent::Action(play, None)]
    );
}

#[test]
fn focus_is_restored_when_a_modal_returns_to_the_previous_screen() {
    let mut menus = Menus::new(motion());
    let parent = menus.push(Layer::Menu, document(None)).unwrap();
    let settings = menus.widget_id(parent, "settings").unwrap();
    let mut input = MenuInput::new();
    let bindings = Bindings::new();
    menus.frame(&input, &bindings, (800.0, 600.0), 0).unwrap();
    input.nav = Some(NavInput::Next);
    menus.frame(&input, &bindings, (800.0, 600.0), 1).unwrap();
    input.nav = None;

    let modal = menus.push(Layer::Modal, document(None)).unwrap();
    let modal_play = menus.widget_id(modal, "play").unwrap();
    input.confirm = true;
    assert_eq!(
        menus.frame(&input, &bindings, (800.0, 600.0), 2).unwrap(),
        [UiEvent::Action(modal_play, None)]
    );
    input.confirm = false;
    menus.frame(&input, &bindings, (800.0, 600.0), 3).unwrap();
    menus.pop(Layer::Modal).unwrap();
    input.confirm = true;
    assert_eq!(
        menus.frame(&input, &bindings, (800.0, 600.0), 4).unwrap(),
        [UiEvent::Action(settings, None)]
    );
}

#[test]
fn repeated_actions_share_a_widget_id_and_emit_their_row() {
    let mut menus = Menus::new(motion());
    let screen = menus.push(Layer::Menu, repeated_document()).unwrap();
    let kick = menus.widget_id(screen, "kick").unwrap();
    let mut bindings = Bindings::new();
    bindings.set_list("seats", seats(&["Ada", "Bryn", "Cy"]));
    let mut input = MenuInput::new();
    menus.frame(&input, &bindings, (800.0, 600.0), 0).unwrap();
    input.nav = Some(NavInput::Prev);
    menus.frame(&input, &bindings, (800.0, 600.0), 1).unwrap();
    input.nav = None;
    input.confirm = true;
    assert_eq!(
        menus.frame(&input, &bindings, (800.0, 600.0), 2).unwrap(),
        [UiEvent::Action(kick, Some(RowIndex(2)))]
    );
}

#[test]
fn repeated_button_hit_testing_uses_the_third_rows_button() {
    let mut menus = Menus::new(motion());
    let screen = menus.push(Layer::Menu, repeated_document()).unwrap();
    let kick = menus.widget_id(screen, "kick").unwrap();
    let mut bindings = Bindings::new();
    bindings.set_list("seats", seats(&["Ada", "Bryn", "Cy"]));
    let mut input = MenuInput::new();
    menus.frame(&input, &bindings, (800.0, 600.0), 0).unwrap();
    let draw = menus.draw_list().next().unwrap();
    let button = draw
        .list
        .widgets()
        .iter()
        .find(|widget| widget.row == Some(RowIndex(2)))
        .unwrap();
    input.pointer.position = glam::Vec2::new(
        button.rect.x + button.rect.w * 0.5,
        button.rect.y + button.rect.h * 0.5,
    );
    input.pointer.pressed = true;
    assert_eq!(
        menus.frame(&input, &bindings, (800.0, 600.0), 1).unwrap(),
        [UiEvent::Action(kick, Some(RowIndex(2)))]
    );
}

#[test]
fn focus_moves_to_the_nearest_surviving_row_then_the_default() {
    let mut menus = Menus::new(motion());
    let screen = menus.push(Layer::Menu, repeated_document()).unwrap();
    let kick = menus.widget_id(screen, "kick").unwrap();
    let cancel = menus.widget_id(screen, "cancel").unwrap();
    let mut bindings = Bindings::new();
    bindings.set_list("seats", seats(&["Ada", "Bryn", "Cy"]));
    let mut input = MenuInput::new();
    menus.frame(&input, &bindings, (800.0, 600.0), 0).unwrap();
    input.nav = Some(NavInput::Prev);
    menus.frame(&input, &bindings, (800.0, 600.0), 1).unwrap();
    input.nav = None;
    bindings.set_list("seats", seats(&["Ada", "Bryn"]));
    menus.frame(&input, &bindings, (800.0, 600.0), 2).unwrap();
    input.confirm = true;
    assert_eq!(
        menus.frame(&input, &bindings, (800.0, 600.0), 3).unwrap(),
        [UiEvent::Action(kick, Some(RowIndex(1)))]
    );
    input.confirm = false;
    menus.frame(&input, &bindings, (800.0, 600.0), 4).unwrap();
    bindings.set_list("seats", seats(&[]));
    menus.frame(&input, &bindings, (800.0, 600.0), 5).unwrap();
    input.confirm = true;
    assert_eq!(
        menus.frame(&input, &bindings, (800.0, 600.0), 6).unwrap(),
        [UiEvent::Action(cancel, None)]
    );
}

#[test]
fn screen_back_metadata_round_trips() {
    assert!(
        matches!(document(Some("play")).screen().unwrap().back, BackRule::Action(ref id) if id == "play")
    );
}
