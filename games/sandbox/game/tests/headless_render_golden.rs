//! Compares retained sandbox scenes against decoded PNG reference frames.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use app::{ClientApp, LookDelta};
use common::content_pack;
use platform::{DeviceClass, DeviceProfile};
use render::golden::{Golden, ImageRegion, Plausible, Tolerance};
use render::{RenderTier, Settings};
use sandbox::{SandboxGame, SandboxPresenter, app_config};

/// Rendered output goes to Cargo's per-target scratch folder, never into the source tree.
fn scratch(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)
}

const SIZE: (u32, u32) = (640, 360);
const TOLERANCE: Tolerance = Tolerance {
    mean_rgb_error: 0.05,
    max_rgb_channel_error: 4,
};

struct SampleGolden {
    class: DeviceClass,
    settings: Settings,
    show_touch_buttons: bool,
    output: &'static str,
    fixture: &'static str,
    label: &'static str,
    camera_offset: LookDelta,
}

#[test]
fn sandbox_matches_main_within_taa_noise_bound() {
    let content = content_pack();
    let path = scratch("sandbox-headless-golden.png");
    let path = path.as_path();
    let mut client = client(
        SandboxGame::from_content(&content),
        DeviceClass::Desktop,
        high_settings(),
        false,
        false,
        LookDelta::ZERO,
    );
    platform::headless::run(&mut client, SIZE, 30, Some(path)).expect("headless render");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/main-headless.png");
    Golden::new("sandbox/main", scene_in_view("sandbox centre"))
        .compare(path, &fixture, TOLERANCE)
        .expect("sandbox main golden");
}

#[test]
fn sample_arena_desktop_matches_its_reference_within_taa_noise_bound() {
    let content = content_pack();
    render_sample_arena(
        SandboxGame::sample_arena(&content),
        SampleGolden {
            class: DeviceClass::Desktop,
            settings: high_settings(),
            show_touch_buttons: false,
            output: "sandbox-sample-arena-desktop-golden.png",
            fixture: "sample-arena-desktop.png",
            label: "sample arena desktop render",
            camera_offset: sample_arena_camera_offset(),
        },
    );
}

#[test]
fn sample_arena_phone_matches_its_reference_within_taa_noise_bound() {
    let content = content_pack();
    render_sample_arena(
        SandboxGame::sample_arena(&content),
        SampleGolden {
            class: DeviceClass::Handheld,
            settings: RenderTier::Low.settings(),
            show_touch_buttons: true,
            output: "sandbox-sample-arena-phone-golden.png",
            fixture: "sample-arena-phone.png",
            label: "sample arena phone render",
            camera_offset: sample_arena_camera_offset(),
        },
    );
}

#[test]
fn sample_arena_lockdown_matches_its_reference_within_taa_noise_bound() {
    let content = content_pack();
    render_sample_arena(
        SandboxGame::sample_arena_lockdown(&content),
        SampleGolden {
            class: DeviceClass::Desktop,
            settings: high_settings(),
            show_touch_buttons: false,
            output: "sandbox-sample-arena-lockdown-golden.png",
            fixture: "sample-arena-lockdown.png",
            label: "sample arena lockdown render",
            camera_offset: sample_arena_camera_offset(),
        },
    );
}

fn render_sample_arena(game: SandboxGame, golden: SampleGolden) {
    let mut client = client(
        game,
        golden.class,
        golden.settings,
        golden.show_touch_buttons,
        false,
        golden.camera_offset,
    );
    let path = scratch(golden.output);
    let path = path.as_path();
    platform::headless::run(&mut client, SIZE, 30, Some(path)).expect("headless render");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(golden.fixture);
    let name = format!("sandbox/{}", golden.fixture.trim_end_matches(".png"));
    Golden::new(name, scene_in_view(golden.label))
        .compare(path, &fixture, TOLERANCE)
        .expect("sample arena golden");
}

fn client(
    game: SandboxGame,
    class: DeviceClass,
    settings: Settings,
    show_touch_buttons: bool,
    show_telemetry: bool,
    camera_offset: LookDelta,
) -> ClientApp<SandboxGame, SandboxPresenter> {
    let profile = DeviceProfile::detect(class, None, show_touch_buttons).expect("device profile");
    let content = content_pack();
    let mut client = ClientApp::new(
        game,
        &(),
        SandboxPresenter::new(show_touch_buttons)
            .with_telemetry(show_telemetry)
            .with_camera_offset(camera_offset),
        &app_config(Some(0)).expect("app config"),
        &profile,
        settings,
    )
    .expect("sandbox client");
    client
        .mount_content(content)
        .expect("mount sandbox content");
    client
}

fn sample_arena_camera_offset() -> LookDelta {
    LookDelta::new(0.0, -0.4).expect("finite sample arena camera pitch")
}

fn high_settings() -> Settings {
    let mut settings = RenderTier::High.settings();
    settings.taa = true;
    settings
}

/// The scene fills the centre of the frame: most of the middle region is not the black clear colour.
fn scene_in_view(label: &'static str) -> Plausible {
    Plausible::not_clear_color(
        label,
        ImageRegion::new(256, 144, 384, 216),
        [0, 0, 0],
        10,
        8,
    )
}
