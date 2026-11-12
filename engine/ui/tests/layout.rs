//! Layout behaviour of UI documents at different screen shapes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use engine_ui::{
    Bindings, Document, DrawList, MetricsKey, Primitive, Rect, TextMeasure, UiError, UiLayout,
    check_interactive_bounds, check_text_overflow,
};
use proptest::prelude::*;

struct Mono;
impl TextMeasure for Mono {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(0)
    }

    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * 0.5 * text.chars().count() as f32
    }
}

const HUD: &str = r##"(version: 1, root: (
  layout: (display: Flex, direction: Column, justify: SpaceBetween, padding: 20.0),
  children: [
    (layout: (display: Flex, direction: Row, gap: 10.0, height: Px(40.0)), children: [
      (id: Some("ammo"), layout: (display: Flex, width: Auto), content: Text(text: "AMMO {ammo:0}", size: 24.0, color: "#ffffff")),
      (id: Some("health"), layout: (display: Flex, grow: 1.0, height: Px(20.0)), style: (fill: Some("#00000080")),
        content: Bar(value: "health", color: "#ff3030")),
    ]),
    (id: Some("warn"), show_if: Some("low"), layout: (display: Flex, height: Px(30.0)),
      content: Text(text: "LOW", size: 20.0, color: "#ff0000")),
    (layout: (display: Flex, position: Absolute(right: Some(Short(0.05)), bottom: Some(Short(0.05)), left: None, top: None),
              width: Short(0.2), height: Short(0.2)),
      content: Button(id: 1, label: "FIRE", color: "#ffffff", held: "fire")),
  ]))"##;

fn bindings(low: bool, fire: bool) -> Bindings {
    let mut b = Bindings::new();
    b.set_num("ammo", 30.0);
    b.set_num("health", 0.5);
    b.set_flag("low", low);
    b.set_flag("fire", fire);
    b
}

fn lay(doc: &Document, b: &Bindings, w: f32, h: f32) -> Result<DrawList, UiError> {
    let mut layout = UiLayout::new(doc.clone())?;
    let list = layout.frame(b, (w, h), &mut Mono)?;
    Ok(list.clone())
}

#[test]
fn retained_frames_match_a_fresh_layout_across_changes() {
    let doc = Document::from_ron(HUD).unwrap();
    let changes = proptest::collection::vec(
        (
            0u8..100,
            any::<bool>(),
            any::<bool>(),
            0u8..100,
            200u16..2000,
            200u16..2000,
        ),
        1..16,
    );
    proptest!(|(changes in changes)| {
        let mut layout = UiLayout::new(doc.clone()).unwrap();
        let mut bindings = Bindings::new();
        let mut measure = Mono;
        for (ammo, low, fire, health, width, height) in &changes {
            bindings.set_num("ammo", f32::from(*ammo));
            bindings.set_num("health", f32::from(*health) / 100.0);
            bindings.set_flag("low", *low);
            bindings.set_flag("fire", *fire);
            let retained_list = layout.frame(&bindings, (f32::from(*width), f32::from(*height)), &mut measure).unwrap().clone();
            let fresh_list = lay(&doc, &bindings, f32::from(*width), f32::from(*height)).unwrap();
            prop_assert_eq!(retained_list, fresh_list);
        }
    });
}

#[test]
fn cached_and_paint_only_frames_do_not_measure_text() {
    struct CountingMeasure {
        key: MetricsKey,
        calls: usize,
    }
    impl TextMeasure for CountingMeasure {
        fn metrics_key(&self) -> MetricsKey {
            self.key
        }
        fn width(&mut self, px: f32, text: &str) -> f32 {
            self.calls += 1;
            px * 0.5 * text.chars().count() as f32
        }
    }

    let doc = Document::from_ron(HUD).unwrap();
    let mut layout = UiLayout::new(doc).unwrap();
    let mut bindings = bindings(false, false);
    let mut measure = CountingMeasure {
        key: MetricsKey(4),
        calls: 0,
    };
    let _ = layout
        .frame(&bindings, (1280.0, 720.0), &mut measure)
        .unwrap();
    let calls = measure.calls;
    bindings.set_num("health", 0.75);
    let _ = layout
        .frame(&bindings, (1280.0, 720.0), &mut measure)
        .unwrap();
    assert_eq!(
        measure.calls, calls,
        "a paint-only bar change measured text"
    );
    let _ = layout
        .frame(&bindings, (1280.0, 720.0), &mut measure)
        .unwrap();
    assert_eq!(measure.calls, calls, "an unchanged frame measured text");
    measure.key = MetricsKey(5);
    let _ = layout
        .frame(&bindings, (1280.0, 720.0), &mut measure)
        .unwrap();
    assert!(
        measure.calls > calls,
        "a metrics identity change did not remeasure text"
    );
}

#[test]
fn the_button_stays_square_and_in_the_corner_on_both_aspects() {
    let doc = Document::from_ron(HUD).unwrap();
    for (w, h) in [(1280.0, 720.0), (1600.0, 720.0)] {
        let list = lay(&doc, &bindings(false, false), w, h).unwrap();
        let r = list.touch_regions()[0].rect.rect();
        assert!((r.w - 0.2 * h).abs() < 1e-4, "width {}", r.w);
        // 5% of the short side from the right and bottom edges, then half the 20% box.
        let cx = w - 0.05 * h - 0.1 * h;
        let cy = h - 0.05 * h - 0.1 * h;
        assert!((r.x + r.w * 0.5 - cx).abs() < 1e-3, "{w}x{h}");
        assert!((r.y + r.h * 0.5 - cy).abs() < 1e-3);
    }
}

#[test]
fn show_if_adds_and_removes_the_node() {
    let doc = Document::from_ron(HUD).unwrap();
    let count = |low| {
        let list = lay(&doc, &bindings(low, false), 1280.0, 720.0).unwrap();
        list.primitives()
            .iter()
            .filter(|p| matches!(p, Primitive::Text { range, .. } if list.text_of(range) == Some("LOW")))
            .count()
    };
    assert_eq!(count(true), 1);
    assert_eq!(count(false), 0);
}

#[test]
fn the_bar_fills_to_its_value() {
    let doc = Document::from_ron(HUD).unwrap();
    let list = lay(&doc, &bindings(false, false), 1280.0, 720.0).unwrap();
    let fills: Vec<Rect> = list
        .primitives()
        .iter()
        .filter_map(|p| match p {
            Primitive::Fill { rect, color, .. } if color.alpha > 0.9 => Some(*rect),
            _ => None,
        })
        .collect();
    let track = list
        .primitives()
        .iter()
        .find_map(|p| match p {
            Primitive::Fill { rect, color, .. } if color.alpha < 0.6 => Some(*rect),
            _ => None,
        })
        .unwrap();
    let bar = fills[0];
    assert!(
        (bar.w - track.w * 0.5).abs() < 1e-3,
        "bar {} track {}",
        bar.w,
        track.w
    );
}

#[test]
fn text_is_formatted_from_bindings_and_sized_by_the_measurer() {
    let doc = Document::from_ron(HUD).unwrap();
    let list = lay(&doc, &bindings(false, false), 1280.0, 720.0).unwrap();
    let (rect, text) = list
        .primitives()
        .iter()
        .find_map(|p| match p {
            Primitive::Text { rect, range, .. } => {
                Some((*rect, list.text_of(range).unwrap().to_owned()))
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(text, "AMMO 30");
    assert!((rect.w - 24.0 * 0.5 * 7.0).abs() < 1e-3);
}

#[test]
fn fixed_text_shrinks_to_its_minimum_then_uses_ellipsis() {
    let doc = Document::from_ron(
        r##"(version: 1, root: (children: [(layout: (display: Flex, width: Px(30.0), height: Px(24.0)), content: Text(text: "LONGTEXT", size: 20.0, color: "#ffffff"))]))"##,
    )
    .unwrap();
    let list = lay(&doc, &Bindings::new(), 1000.0, 1000.0).unwrap();
    let text = list
        .primitives()
        .iter()
        .find_map(|primitive| match primitive {
            Primitive::Text { px, range, .. } => Some((*px, list.text_of(range))),
            _ => None,
        })
        .unwrap();
    assert!(
        (text.0 - 20.8333).abs() < 0.01,
        "font px {} text {:?}",
        text.0,
        text.1
    );
    assert_eq!(text.1, Some("LON…"));
    check_text_overflow(&list).unwrap();
}

#[test]
fn a_held_button_draws_stronger() {
    let doc = Document::from_ron(HUD).unwrap();
    let alpha = |held| {
        let list = lay(&doc, &bindings(false, held), 1280.0, 720.0).unwrap();
        list.primitives()
            .iter()
            .find_map(|p| match p {
                Primitive::Disc { color, .. } => Some(color.alpha),
                _ => None,
            })
            .unwrap()
    };
    assert!(alpha(true) > alpha(false));
}

#[test]
fn missing_and_mistyped_variables_are_errors() {
    let doc = Document::from_ron(HUD).unwrap();
    let mut b = bindings(false, false);
    b.set_text("health", "x");
    assert!(matches!(
        lay(&doc, &b, 1280.0, 720.0),
        Err(UiError::BindingType { node, .. }) if node == "root/0/1"
    ));
    assert!(matches!(
        lay(&doc, &Bindings::new(), 1280.0, 720.0),
        Err(UiError::UnboundVariable { node, .. }) if node == "root/0/0"
    ));
}

#[test]
fn a_bad_screen_is_an_error() {
    let doc = Document::from_ron(HUD).unwrap();
    assert!(matches!(
        lay(&doc, &bindings(false, false), 0.0, 720.0),
        Err(UiError::Layout(_))
    ));
    assert!(matches!(
        lay(&doc, &bindings(false, false), f32::NAN, 720.0),
        Err(UiError::Layout(_))
    ));
}

proptest! {
    #[test]
    fn every_rectangle_is_finite_and_buttons_are_on_screen(w in 200.0f32..4000.0, h in 200.0f32..4000.0, low: bool, fire: bool) {
        let doc = Document::from_ron(HUD).unwrap();
        let list = lay(&doc, &bindings(low, fire), w, h).unwrap();
        for p in list.primitives() {
            if let Primitive::Fill { rect, .. } | Primitive::Text { rect, .. } = p {
                prop_assert!(rect.x.is_finite() && rect.y.is_finite() && rect.w >= 0.0 && rect.h >= 0.0);
            }
        }
        for region in list.touch_regions() {
            let rect = region.rect.rect();
            prop_assert!(rect.x >= 0.0 && rect.y >= 0.0 && rect.x + rect.w <= w && rect.y + rect.h <= h);
        }
    }
}

#[test]
fn interactive_bounds_reject_clipped_touch_controls_and_accept_in_bounds_controls() {
    let doc = Document::from_ron(HUD).unwrap();
    let list = lay(&doc, &bindings(false, false), 1280.0, 720.0).unwrap();
    assert_eq!(
        list.touch_regions().len(),
        1,
        "the control is installed at runtime"
    );
    assert!(matches!(
        check_interactive_bounds(
            &list,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1000.0,
                h: 720.0
            },
        ),
        Err(UiError::InteractiveOutsideSafeArea {
            kind: "touch control",
            id: 1
        })
    ));

    let list = lay(
        &Document::from_ron(HUD).unwrap(),
        &bindings(false, false),
        1280.0,
        720.0,
    )
    .unwrap();
    assert!(
        check_interactive_bounds(
            &list,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1280.0,
                h: 720.0
            },
        )
        .is_ok()
    );
}

#[test]
fn a_document_survives_a_trip_through_text() {
    let doc = Document::from_ron(HUD).unwrap();
    let again = Document::from_ron(&doc.to_ron().unwrap()).unwrap();
    assert_eq!(doc, again);
}

#[test]
fn a_skewed_node_brackets_its_subtree_and_leans_about_its_middle() {
    let doc = Document::from_ron(
        r##"(version: 1, root: (layout: (display: Flex), children: [
          (layout: (display: Flex, height: Px(100.0)), style: (skew: -10.0), children: [
            (layout: (display: Flex, height: Px(40.0)), style: (fill: Some("#ffffff"))),
          ]),
          (layout: (display: Flex, height: Px(40.0)), style: (fill: Some("#ff0000"))),
        ]))"##,
    )
    .unwrap();
    let list = lay(&doc, &Bindings::new(), 400.0, 400.0).unwrap();
    let prims = list.primitives();
    let Primitive::Skew(Some(skew)) = &prims[0] else {
        unreachable!("expected a skew first, got {:?}", prims[0]);
    };
    assert!((skew.lean - 10.0_f32.to_radians().tan()).abs() < 1e-6);
    let middle = 100.0 * (400.0 / 720.0) * 0.5;
    assert!(
        (skew.pivot_y - middle).abs() <= 0.5, // taffy rounds boxes to whole pixels
        "pivot {} vs {middle}",
        skew.pivot_y
    );
    assert!(matches!(prims[1], Primitive::Fill { .. }));
    assert_eq!(
        prims[2],
        Primitive::Skew(None),
        "restored after the subtree"
    );
    assert!(matches!(prims[3], Primitive::Fill { .. }));
}

#[test]
fn a_skew_outside_the_allowed_range_is_rejected() {
    let text = r##"(version: 1, root: (style: (skew: 60.0)))"##;
    assert!(Document::from_ron(text).is_err());
}
