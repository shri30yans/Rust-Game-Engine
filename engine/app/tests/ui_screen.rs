//! `UiScreen` draws pack text and reloads it, keeping the last good one on errors.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use app::{AppError, UiScreen};
use engine_ui::Bindings;
use render::{FontId, Ui};

const ONE_BOX: &str = r##"(version: 1, root: (children: [(layout: (width: Px(100.0), height: Px(50.0)), style: (fill: Some("#ff0000")))]))"##;
const TWO_BOXES: &str = r##"(version: 1, root: (children: [
  (layout: (width: Px(100.0), height: Px(50.0)), style: (fill: Some("#ff0000"))),
  (layout: (width: Px(100.0), height: Px(50.0)), style: (fill: Some("#00ff00")))]))"##;

fn verts(screen: &mut UiScreen) -> usize {
    let mut ui = Ui::new();
    ui.size = glam::Vec2::new(1280.0, 720.0);
    screen
        .draw(&mut ui, FontId(0), &Bindings::new(), (1280.0, 720.0))
        .unwrap();
    ui.verts.len()
}

#[test]
fn a_document_draws_its_boxes() {
    let mut one = UiScreen::from_source(ONE_BOX).unwrap();
    let mut two = UiScreen::from_source(TWO_BOXES).unwrap();
    let n = verts(&mut one);
    assert!(n > 0);
    assert_eq!(verts(&mut two), 2 * n);
}

#[test]
fn changed_pack_text_reloads_and_a_broken_one_keeps_the_last_good_document() {
    let mut screen = UiScreen::from_source(ONE_BOX).unwrap();
    let one = verts(&mut screen);
    screen.reload_source(TWO_BOXES).unwrap();
    assert_eq!(verts(&mut screen), 2 * one);

    let err = screen.reload_source("(version: 1, root: (bogus: 1))");
    assert!(matches!(err, Err(AppError::Ui(_))));
    assert_eq!(verts(&mut screen), 2 * one, "the last good document stays");

    screen.reload_source(ONE_BOX).unwrap();
    assert_eq!(verts(&mut screen), one);
}

#[test]
fn a_failed_layout_replays_the_last_good_draw_list() {
    let document = r##"(version: 1, root: (children: [
      (layout: (width: Px(100.0), height: Px(50.0)), style: (fill: Some("#ff0000")),
       content: Text(text: "{label}", size: 20.0, color: "#ffffff"))]))"##;
    let mut screen = UiScreen::from_source(document).unwrap();
    let mut bindings = Bindings::new();
    bindings.set_text("label", "READY");
    let mut ui = Ui::new();
    ui.size = glam::Vec2::new(1280.0, 720.0);
    let font = ui
        .atlas
        .add_font(include_bytes!("../../render/fonts/Roboto-Medium.ttf"))
        .unwrap();
    screen
        .draw(&mut ui, font, &bindings, (1280.0, 720.0))
        .unwrap();
    let good_vertices = ui.verts.len();

    let bad = Bindings::new();
    ui.clear();
    assert!(matches!(
        screen.draw(&mut ui, font, &bad, (1280.0, 720.0)),
        Err(AppError::Ui(engine_ui::UiError::UnboundVariable { .. }))
    ));
    assert_eq!(
        ui.verts.len(),
        good_vertices,
        "failed layout replays the good HUD"
    );
}

#[test]
fn invalid_pack_text_is_a_typed_ui_error() {
    let err = UiScreen::from_source("not a document");
    assert!(matches!(err, Err(AppError::Ui(_))));
}
