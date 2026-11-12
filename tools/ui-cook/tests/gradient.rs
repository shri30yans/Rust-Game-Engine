//! CSS `linear-gradient` cooks into the document's gradient background, and a malformed gradient
//! is a cook error that names the property.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use engine_ui::{Background, Color, Document, GradientStop};
use ui_cook::cook;

const GRADIENT_PAGE: &str = include_str!("fixtures/gradient.html");

fn stop(hex: &str, pos: f32) -> GradientStop {
    GradientStop {
        pos,
        color: Color::parse(hex).unwrap(),
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-6
}

fn page_with_background(value: &str) -> String {
    format!(
        "<!doctype html><html><head><style>#bar {{ width: 10px; height: 10px; background: {value}; }}</style></head>\
         <body><div id=\"bar\"></div></body></html>"
    )
}

#[test]
fn a_css_gradient_cooks_into_the_background_brush() {
    let ron = cook(GRADIENT_PAGE).unwrap();
    let doc = Document::from_ron(&ron).unwrap();
    let bar = &doc.root().children[0];
    let Background::LinearGradient { angle_deg, stops } = &bar.style.background else {
        panic!("expected a gradient, got {:?}", bar.style.background);
    };
    assert!(near(*angle_deg, 90.0));
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0], stop("#000000", 0.0));
    assert!(near(stops[1].pos, 0.4));
    assert_eq!(stops[1].color, Color::parse("#808080").unwrap());
    assert_eq!(stops[2], stop("#ffffff", 1.0));
}

#[test]
fn a_second_gradient_keeps_its_own_angle() {
    let ron = cook(GRADIENT_PAGE).unwrap();
    let doc = Document::from_ron(&ron).unwrap();
    let glow = &doc.root().children[1];
    let Background::LinearGradient { angle_deg, stops } = &glow.style.background else {
        panic!("expected a gradient, got {:?}", glow.style.background);
    };
    assert!(near(*angle_deg, 0.0));
    assert_eq!(stops, &vec![stop("#102030", 0.0), stop("#ff6a1a", 1.0)]);
}

#[test]
fn the_cooked_gradient_document_round_trips() {
    let doc = Document::from_ron(&cook(GRADIENT_PAGE).unwrap()).unwrap();
    assert_eq!(Document::from_ron(&doc.to_ron().unwrap()).unwrap(), doc);
}

#[test]
fn a_malformed_gradient_is_a_cook_error_naming_the_background_property() {
    let cases = [
        // One stop: a gradient needs two.
        ("linear-gradient(90deg, #000000 0%)", "background"),
        // Five stops: four is the limit.
        (
            "linear-gradient(90deg, #000 0%, #111 20%, #222 40%, #333 60%, #444 100%)",
            "background",
        ),
        // Positions out of order.
        ("linear-gradient(90deg, #000 80%, #fff 20%)", "background"),
        // A position over 100%.
        ("linear-gradient(90deg, #000 0%, #fff 120%)", "background"),
        // An angle that is not in degrees.
        ("linear-gradient(1rad, #000 0%, #fff 100%)", "background"),
        // A stop without a percentage position.
        ("linear-gradient(90deg, #000 0%, #fff)", "percentage"),
        // A colour that is not hex.
        (
            "linear-gradient(90deg, #zzzzzz 0%, #fff 100%)",
            "background",
        ),
        // Something other than a gradient.
        ("#ff0000", "background"),
    ];
    for (value, expected) in cases {
        let error = cook(&page_with_background(value)).expect_err(value);
        let message = error.to_string();
        assert!(message.contains("background"), "{value}: {message}");
        assert!(message.contains(expected), "{value}: {message}");
    }
}
