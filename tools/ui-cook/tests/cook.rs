//! The cooker turns a page into a document that lays out like the hand-written one.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use engine_ui::{Bindings, Document, MetricsKey, Primitive, TextMeasure, UiLayout};
use ui_cook::{CookError, cook};

const HUD: &str = include_str!("../examples/hud.html");

struct Mono;
impl TextMeasure for Mono {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(0)
    }

    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * 0.5 * text.chars().count() as f32
    }
}

fn bindings(low: bool) -> Bindings {
    let mut b = Bindings::new();
    b.set_num("ammo", 30.0);
    b.set_num("health", 0.5);
    b.set_flag("low", low);
    b.set_flag("fire", false);
    b
}

#[test]
fn the_example_compiles_to_a_valid_document_that_round_trips() {
    let ron = cook(HUD).unwrap();
    let doc = Document::from_ron(&ron).unwrap();
    assert_eq!(Document::from_ron(&doc.to_ron().unwrap()).unwrap(), doc);
}

#[test]
fn menu_html_elements_states_and_screen_rules_round_trip() {
    let page = r##"<style>
        button:hover { background-color: #101010; }
        button:focus-visible { border: 3px solid #ffffff; }
        button:active { opacity: 0.8; transform: scale(0.97); }
        button:disabled { color: #888888; }
    </style>
    <body data-default-focus="play" data-back="play">
      <button id="play" data-action="play">PLAY</button>
      <input id="music" type="checkbox" data-bind="music" aria-label="Music" style="width: 48px; height: 48px">
      <input id="volume" type="range" min="0" max="1" step="0.1" data-bind="volume" aria-label="Volume" style="width: 180px; height: 48px">
      <select id="quality" data-bind="quality-index" data-options="quality" style="width: 180px; height: 48px">Quality</select>
      <div role="tablist" id="sections" data-bind="tab-index" data-options="tabs"></div>
      <template data-repeat="items" data-max="3"><p style="font-size: 16px; color: #ffffff">{item.name}</p></template>
    </body>"##;
    let ron = cook(page).unwrap();
    let document = Document::from_ron(&ron).unwrap();
    assert_eq!(
        Document::from_ron(&document.to_ron().unwrap()).unwrap(),
        document
    );
    let screen = document.screen().unwrap();
    assert_eq!(screen.default_focus.as_deref(), Some("play"));
    assert!(matches!(screen.back, engine_ui::BackRule::Action(ref id) if id == "play"));
    assert!(matches!(
        document.root().children[0].content,
        engine_ui::doc::Content::Action { .. }
    ));
    assert!(document.root().children[0].style.states.hover.is_some());
    assert!(document.root().children[0].style.states.focus.is_some());
    assert!(document.root().children[0].style.states.pressed.is_some());
    assert!(document.root().children[0].style.states.disabled.is_some());
}

#[test]
fn text_fit_and_overflow_rules_round_trip_and_reject_bad_values() {
    let encoded = cook(
        r##"<body><p style="font-size: 20px; color: #ffffff; fit: shrink 80%; text-overflow: ellipsis">PLAY</p></body>"##,
    )
    .expect("text fit styles are supported");
    let document = engine_ui::Document::from_ron(&encoded).expect("cooked document parses");
    assert!(
        cook(r#"<body><p style="font-size: 20px; color: #ffffff; fit: stretch">PLAY</p></body>"#)
            .is_err()
    );
    assert!(cook(
        r#"<body><p style="font-size: 20px; color: #ffffff; text-overflow: clip">PLAY</p></body>"#
    )
    .is_err());

    assert_eq!(
        document,
        Document::from_ron(&encoded).expect("document round trips")
    );
}

#[test]
fn the_compiled_page_lays_out_like_the_design() {
    let doc = Document::from_ron(&cook(HUD).unwrap()).unwrap();
    let mut layout = UiLayout::new(doc).unwrap();
    let mut bindings = bindings(true);
    let list = layout.frame(&bindings, (1280.0, 720.0), &mut Mono).unwrap();
    let button = list.touch_regions()[0];
    assert_eq!(button.id.0, 1);
    assert_eq!(button.rect.rect().w, 144.0);
    let texts: Vec<&str> = list
        .primitives()
        .iter()
        .filter_map(|p| match p {
            Primitive::Text { range, .. } => list.text_of(range),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["AMMO 30", "LOW", "FIRE"]);
    // The warning leaves when its flag is off.
    bindings.set_flag("low", false);
    let list = layout.frame(&bindings, (1280.0, 720.0), &mut Mono).unwrap();
    let n = list
        .primitives()
        .iter()
        .filter(|p| matches!(p, Primitive::Text { .. }))
        .count();
    assert_eq!(n, 2);
}

#[test]
fn id_beats_class_beats_tag_and_inline_wins() {
    let page = |extra: &str| {
        format!(
            r##"<style>p {{ font-size: 10px; color: #ffffff; }} .a {{ font-size: 20px; }} #x {{ font-size: 30px; }}</style>
            <body><p id="x" class="a" {extra}>hi</p></body>"##
        )
    };
    let size = |html: &str| {
        let doc = Document::from_ron(&cook(html).unwrap()).unwrap();
        match &doc.root().children[0].content {
            engine_ui::doc::Content::Text { size, .. } => *size,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(size(&page("")), 30.0);
    assert_eq!(size(&page(r#"style="font-size: 40px""#)), 40.0);
}

fn fails(body: &str) -> CookError {
    cook(&format!("<body>{body}</body>")).unwrap_err()
}

#[test]
fn anything_outside_the_subset_is_a_named_error() {
    assert!(matches!(fails("<img>"), CookError::UnsupportedElement(t) if t == "img"));
    assert!(matches!(
        fails(r#"<div onclick="x"></div>"#),
        CookError::UnsupportedAttribute { .. }
    ));
    assert!(matches!(
        fails(r#"<div style="float: left"></div>"#),
        CookError::UnsupportedProperty { .. }
    ));
    assert!(matches!(
        fails(r#"<div style="display: grid"></div>"#),
        CookError::BadValue { .. }
    ));
    assert!(matches!(
        fails(r#"<div style="flex-direction: column"></div>"#),
        CookError::BadValue { property, .. } if property == "flex-direction"
    ));
    assert!(matches!(
        fails(r#"<div style="left: 1px"></div>"#),
        CookError::BadValue { .. }
    ));
    assert!(matches!(
        fails("<div>loose</div>"),
        CookError::Structure { .. }
    ));
    assert!(matches!(
        fails(r#"<p style="color:#ffffff">x</p>"#),
        CookError::MissingProperty { .. }
    ));
    assert!(matches!(
        fails(r#"<progress style="color:#ffffff"></progress>"#),
        CookError::MissingAttribute { .. }
    ));
    assert!(matches!(
        fails(r#"<div style="color:#ffffff"></div>"#),
        CookError::UnsupportedProperty { .. }
    ));
    assert!(matches!(
        fails(r#"<p style="font-size:10px;color:#ffffff">{</p>"#),
        CookError::Ui(_)
    ));
    assert!(matches!(
        fails(r#"<button data-id="300" data-held="h" style="color:#ffffff">x</button>"#),
        CookError::BadValue { .. }
    ));
}

#[test]
fn a_visible_bar_with_no_height_is_rejected_by_the_layout_check() {
    let document = Document::from_ron(
        &cook(r#"<body><progress data-bind="health" style="width: 100px; color: #ffffff"></progress></body>"#)
            .unwrap(),
    )
    .unwrap();
    let mut layout = UiLayout::new(document).unwrap();
    let mut values = Bindings::new();
    values.set_num("health", 1.0);
    layout.frame(&values, (1280.0, 720.0), &mut Mono).unwrap();
    assert!(matches!(
        layout.check_visible_content(),
        Err(engine_ui::UiError::ZeroSizeContent { .. })
    ));
}

#[test]
fn a_style_error_in_the_sheet_is_reported() {
    let err = cook("<style>div p { gap: 1px }</style><body></body>").unwrap_err();
    assert!(matches!(err, CookError::UnsupportedSelector(_)));
}

#[test]
fn transform_skew_x_becomes_the_style_skew_and_other_transforms_are_refused() {
    let page = |t: &str| {
        format!(
            "<html><head><style>.a {{ transform: {t}; height: 10px; }}</style></head><body><div class=\"a\"></div></body></html>"
        )
    };
    let ron = cook(&page("skewX(-10deg)")).unwrap();
    assert!(
        ron.contains("skew:-10") || ron.contains("skew: -10"),
        "{ron}"
    );
    assert!(cook(&page("rotate(10deg)")).is_err());
    assert!(cook(&page("skewX(10px)")).is_err());
}

/// Every `games/<name>/ui/<page>.html` has a committed `<page>.ui.ron` that is exactly what the
/// cooker makes of it today, so a page edit cannot reach a build without being re-cooked.
#[test]
fn every_committed_document_is_the_cooked_page() {
    let games = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games");
    let mut checked = 0;
    for game in std::fs::read_dir(games).unwrap() {
        let ui = game.unwrap().path().join("ui");
        let Ok(pages) = std::fs::read_dir(&ui) else {
            continue;
        };
        for page in pages {
            let page = page.unwrap().path();
            if page.extension().is_some_and(|e| e == "html") {
                let cooked = cook(&std::fs::read_to_string(&page).unwrap()).unwrap();
                let committed = std::fs::read_to_string(page.with_extension("ui.ron")).unwrap();
                assert_eq!(
                    committed,
                    cooked,
                    "{} is stale: run ui-cook on it again",
                    page.display()
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "no pages found under games/*/ui");
}
