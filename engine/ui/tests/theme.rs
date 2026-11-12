//! Integration tests for theme loading and validation.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_ui::{ColorId, Srgb8, Theme, ThemeSerial, UiError};

const FIXTURE: &str = include_str!("fixtures/theme.ron");

fn load_fixture(serial: u32) -> Theme {
    Theme::from_ron(FIXTURE, ThemeSerial(serial)).expect("fixture theme")
}

#[test]
fn fixture_resolves_every_token_name() {
    let theme = load_fixture(1);
    assert_eq!(theme.name(), "fixture");

    let text = theme.color_id("text").unwrap();
    let _ = theme.color(text).unwrap();

    let _ = theme.type_id("body").unwrap();
    let _ = theme.type_id("title").unwrap();
    let _ = theme.space_id("xs").unwrap();
    let _ = theme.space_id("sm").unwrap();
    let _ = theme.space_id("md").unwrap();
    let _ = theme.radius_id("sm").unwrap();
    let _ = theme.radius_id("md").unwrap();
    let _ = theme.size_id("cut-s").unwrap();
    let _ = theme.size_id("cut-m").unwrap();
    let _ = theme.glyph_id("star").unwrap();
    let _ = theme.glyph_id("bullet").unwrap();
    let _ = theme.motion_id("fast").unwrap();
}

#[test]
fn contrast_matches_hand_computed_wcag() {
    let theme = load_fixture(1);
    let text = theme.color(theme.color_id("text").unwrap()).unwrap();
    let surface = theme.color(theme.color_id("surface").unwrap()).unwrap();
    let ratio = Srgb8::contrast_ratio(text, surface);
    // #F3F5F8 on #0C0F14 ≈ 17.57 (WCAG relative luminance).
    assert!((ratio - 17.57).abs() <= 0.05, "ratio was {ratio}");
}

#[test]
fn failing_contrast_rule_returns_violation() {
    let mut broken = FIXTURE.to_owned();
    broken = broken.replace(
        "(\"muted\", \"surface\", 3.0)",
        "(\"muted\", \"surface\", 99.0)",
    );
    let err = Theme::from_ron(&broken, ThemeSerial(2)).unwrap_err();
    assert!(matches!(err, UiError::ContrastViolation { .. }));
}

#[test]
fn unknown_section_fails_parse() {
    let broken = FIXTURE.replace("    motion: {", "    unknown_block: 1,\n    motion: {");
    let err = Theme::from_ron(&broken, ThemeSerial(3)).unwrap_err();
    assert!(matches!(err, UiError::Parse(_)));
}

#[test]
fn unknown_color_in_rule_fails() {
    let broken = FIXTURE.replace(
        "(\"text\", \"surface\", 4.5)",
        "(\"text\", \"missing\", 4.5)",
    );
    let err = Theme::from_ron(&broken, ThemeSerial(4)).unwrap_err();
    assert!(matches!(
        err,
        UiError::UnknownToken {
            group: "colors",
            ..
        }
    ));
}

#[test]
fn bad_hex_returns_invalid_color() {
    let broken = FIXTURE.replace("\"text\": \"#F3F5F8\"", "\"text\": \"not-a-color\"");
    let err = Theme::from_ron(&broken, ThemeSerial(5)).unwrap_err();
    assert!(matches!(err, UiError::InvalidColor(_)));
}

#[test]
fn foreign_id_when_theme_mismatch() {
    let a = load_fixture(10);
    let b = load_fixture(11);
    let id: ColorId = a.color_id("text").unwrap();
    let err = b.color(id).unwrap_err();
    assert_eq!(err, UiError::ForeignId);
}

#[test]
fn unknown_color_name_at_resolve() {
    let theme = load_fixture(6);
    let err = theme.color_id("nope").unwrap_err();
    assert!(matches!(
        err,
        UiError::UnknownToken {
            group: "colors",
            ..
        }
    ));
}
