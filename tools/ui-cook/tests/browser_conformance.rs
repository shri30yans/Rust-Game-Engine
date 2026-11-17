//! Compares every Imagine screen with headless Chrome at three device sizes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use engine_ui::{Bindings, Document, Primitive, UiLayout};
use serde_json::json;

#[path = "browser_conformance/samples.rs"]
mod browser_conformance_samples;
use browser_conformance_samples::{
    Mono, ScreenSample, is_text_source, repeat_sources, sample_bindings,
};

const VIEWPORTS: [(u32, u32); 3] = [(1280, 720), (1920, 1080), (2800, 1280)];

#[test]
fn every_imagine_screen_matches_chrome_at_all_reference_viewports() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui = root.join("../../games/imagine/ui");
    let chrome_script = root.join("tests/browser_conformance.mjs");
    let ua_css = root.join("ua.css");
    let mut cases = Vec::new();
    let mut pages = fs::read_dir(&ui)
        .expect("Imagine UI directory reads")
        .map(|entry| entry.expect("UI entry reads").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "html")
        })
        .collect::<Vec<_>>();
    pages.sort();

    for page in pages {
        let screen = page.file_stem().and_then(|name| name.to_str()).unwrap();
        let sample_path = ui.join(format!("{screen}.sample.ron"));
        let sample: ScreenSample =
            ron::from_str(&fs::read_to_string(&sample_path).expect("screen sample bindings read"))
                .unwrap_or_else(|error| panic!("{}: {error}", sample_path.display()));
        let html = fs::read_to_string(&page).expect("screen HTML reads");
        let document = Document::from_ron(&ui_cook::cook(&html).expect("screen cooks"))
            .expect("cooked screen validates");
        let (mut bindings, browser_sample) = sample_bindings(document.root(), &sample);
        let repeat_sources = repeat_sources(document.root());
        let mut layout = UiLayout::new(document).expect("screen layout builds");
        let mut sizes = Vec::new();

        for (width, height) in VIEWPORTS {
            layout
                .frame(&bindings, (width as f32, height as f32), &mut Mono)
                .expect("screen lays out");
            layout
                .check_visible_content()
                .unwrap_or_else(|error| panic!("{screen} at {width}×{height}: {error}"));
            let rects = layout
                .node_rects()
                .expect("screen node rectangles are available")
                .into_iter()
                .filter(|node| node.visible && !repeat_sources.contains(&node.source))
                .map(|node| {
                    let text = is_text_source(&node.source);
                    (
                        node.source,
                        json!({
                            "x": node.rect.x,
                            "y": node.rect.y,
                            "w": node.rect.w,
                            "h": node.rect.h,
                            "text": text,
                        }),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            sizes.push(json!({
                "physicalWidth": width,
                "physicalHeight": height,
                "rects": rects,
            }));
        }
        if screen == "hud" {
            assert_hud_bar_fill(&mut layout, &mut bindings);
        }
        cases.push(json!({
            "screen": screen,
            "html": page,
            "sample": browser_sample,
            "sizes": sizes,
        }));
    }

    assert_eq!(cases.len(), 12, "all twelve Imagine screens are covered");
    let expected_path =
        std::env::temp_dir().join(format!("ui-conform-{}.json", std::process::id()));
    fs::write(
        &expected_path,
        serde_json::to_vec(&json!({ "cases": cases })).expect("expected rectangles encode"),
    )
    .expect("expected rectangles write");
    let status = Command::new("node")
        .arg(chrome_script)
        .arg(ui)
        .arg(ua_css)
        .arg(&expected_path)
        .status();
    let _ = fs::remove_file(&expected_path);
    let status = match status {
        Ok(status) => status,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("SKIPPED browser conformance: Node.js is not installed");
            return;
        }
        Err(error) => panic!("could not start browser harness: {error}"),
    };
    assert!(
        status.success(),
        "Imagine screen browser conformance failed"
    );
}

fn assert_hud_bar_fill(layout: &mut UiLayout, bindings: &mut Bindings) {
    let mut measurements = Vec::new();
    for (health, fraction, low) in [("100", 1.0, false), ("25", 0.25, true)] {
        bindings.set_text("health", health);
        bindings.set_text("max_health", "100");
        bindings.set_num("health_frac", fraction);
        bindings.set_flag("low", low);
        layout
            .frame(bindings, (1280.0, 720.0), &mut Mono)
            .expect("HUD health bar lays out");
        let rects = layout.node_rects().expect("HUD rectangles are available");
        let draw = layout.draw_list_ref().expect("HUD draw list exists");
        let bar = rects
            .iter()
            .find(|node| {
                node.visible
                    && node
                        .source
                        .ends_with(&format!("progress.bar:{}", u8::from(low)))
            })
            .expect("the active health bar is visible");
        let track = rects
            .iter()
            .find(|node| node.source.ends_with("div.track:0"))
            .expect("health bar track exists");
        let fill_width = draw
            .primitives()
            .iter()
            .find_map(|primitive| match primitive {
                Primitive::Fill { rect, color, .. }
                    if color.rgb.r == if low { 0xff } else { 0xf3 }
                        && color.rgb.g == if low { 0x3d } else { 0xf5 }
                        && color.rgb.b == if low { 0x52 } else { 0xf8 }
                        && rect.w > if low { 0.0 } else { 100.0 } =>
                {
                    Some(rect.w)
                }
                _ => None,
            })
            .expect("health fill primitive exists");
        measurements.push((bar.rect.w, fill_width));
        println!(
            "HUD health bar: {health}/100 fills {fill_width:.0}px of {:.0}px bar box ({:.0}px track box)",
            bar.rect.w, track.rect.w
        );
    }
    let [(full_box, full_fill), (quarter_box, quarter_fill)] = measurements.as_slice() else {
        panic!("HUD health bar measurements are complete");
    };
    assert!((full_box - quarter_box).abs() < 0.01);
    assert!((full_fill * 0.25 - quarter_fill).abs() < 0.01);
}
