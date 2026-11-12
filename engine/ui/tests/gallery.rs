//! Golden draw lists for the ui-kit-2 gallery document: a linear gradient and a solid box.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::PathBuf;

use engine_ui::doc::{LayoutDef, Len, Node, StyleDef};
use engine_ui::{
    Background, Bindings, Color, Document, GradientStop, MetricsKey, Srgb8, TextMeasure, UiLayout,
};

struct Mono;
impl TextMeasure for Mono {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(0)
    }
    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * 0.45 * text.chars().count() as f32
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color {
        rgb: Srgb8 { r, g, b },
        a: 255,
    }
}

fn gallery_document() -> Document {
    let gradient = Background::LinearGradient {
        angle_deg: 90.0,
        stops: vec![
            GradientStop {
                pos: 0.0,
                color: rgb(20, 20, 30),
            },
            GradientStop {
                pos: 1.0,
                color: rgb(255, 106, 26),
            },
        ],
    };
    let sized = |w: f32, h: f32| LayoutDef {
        display: engine_ui::doc::Display::Flex,
        width: Len::Px(w),
        height: Len::Px(h),
        ..LayoutDef::default()
    };
    let root = Node {
        layout: LayoutDef {
            display: engine_ui::doc::Display::Flex,
            width: Len::Percent(1.0),
            height: Len::Percent(1.0),
            ..LayoutDef::default()
        },
        children: vec![
            Node {
                id: Some("gradient".to_owned()),
                style: StyleDef {
                    background: gradient,
                    ..StyleDef::default()
                },
                layout: sized(200.0, 80.0),
                ..Node::default()
            },
            Node {
                id: Some("box".to_owned()),
                style: StyleDef {
                    fill: Some(rgb(40, 180, 90)),
                    ..StyleDef::default()
                },
                layout: sized(64.0, 64.0),
                ..Node::default()
            },
        ],
        ..Node::default()
    };
    Document::new(root).unwrap()
}

fn snapshot(size: (f32, f32)) -> String {
    let mut layout = UiLayout::new(gallery_document()).unwrap();
    let list = layout.frame(&Bindings::new(), size, &mut Mono).unwrap();
    format!("{list:#?}\n")
}

fn check_golden(name: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/gallery")
        .join(name);
    if std::env::var_os("UPDATE_GALLERY_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(actual, expected, "golden changed: {}", path.display());
}

#[test]
fn gallery_emits_a_gradient_and_a_box() {
    let text = snapshot((2400.0, 1080.0));
    assert!(text.contains("LinearGradient"));
    assert!(text.contains("Fill"));
}

#[test]
fn gallery_matches_desktop_and_phone_goldens() {
    check_golden("gallery-2400x1080.golden", &snapshot((2400.0, 1080.0)));
    check_golden("gallery-1920x1080.golden", &snapshot((1920.0, 1080.0)));
}
