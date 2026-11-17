//! Compares the retained IMAGINE menu and combat scenes against reference frames.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use app::{ClientApp, GameEntry};
use engine_assets::{ContentPack, PackReader};
use glam::Vec2;
use imagine::flow::Flow;
use imagine::rules::Actors;
use imagine::session::COUNTDOWN_TICKS;
use imagine::sim::ImagineSettings;
use imagine::{ArenaPresenter, ImagineEntry, ImagineGame, app_config};
use platform::{Code, DeviceClass, DeviceProfile, Input, Mouse};
use render::golden::{Golden, Image, ImageRegion, Plausible, Tolerance};
use render::{RenderTier, ViewMode};

/// Rendered output goes to Cargo's per-target scratch folder, never into the source tree.
fn scratch(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)
}

const SIZE: (u32, u32) = (640, 360);
const COMBAT_SIZE: (u32, u32) = (1280, 720);
const FRAME_TICK: f32 = 1.0 / 60.0;
const MENU_SIZE: Vec2 = Vec2::new(1920.0, 1080.0);
const TOLERANCE: Tolerance = Tolerance {
    mean_rgb_error: 0.05,
    max_rgb_channel_error: 4,
};

fn content_pack() -> ContentPack {
    let stream = platform::ContentSource::headless()
        .open_content(ImagineEntry::CONTENT.as_str())
        .expect("IMAGINE pack opens");
    PackReader::read(stream).expect("IMAGINE pack validates")
}

#[test]
fn imagine_menu_backdrop_matches_its_reference_within_taa_noise_bound() {
    let path = scratch("imagine-headless-golden.png");
    let path = path.as_path();
    let profile =
        DeviceProfile::detect(DeviceClass::Desktop, None, false).expect("desktop profile");
    let mut settings = RenderTier::High.settings();
    settings.taa = true;
    let content = content_pack();
    let mut client = ClientApp::new(
        ImagineGame::from_content(&content),
        &ImagineSettings::default(),
        ArenaPresenter::new(&content, false).expect("arena presenter"),
        &app_config(Some(0)).expect("app config"),
        &profile,
        settings,
    )
    .expect("IMAGINE client");
    client
        .mount_content(content)
        .expect("mount IMAGINE content");
    platform::headless::run(&mut client, SIZE, 30, Some(path)).expect("headless render");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/menu-backdrop.png");
    Golden::new(
        "imagine/menu-backdrop",
        Plausible::not_clear_color(
            "title",
            ImageRegion::new(240, 135, 400, 225),
            [4, 6, 12],
            5,
            8,
        ),
    )
    .compare(path, &fixture, TOLERANCE)
    .expect("menu golden");
}

#[test]
fn imagine_cooked_viewmodel_matches_its_reference_within_taa_noise_bound() {
    let path = scratch("imagine-viewmodel-golden.png");
    let path = path.as_path();
    let mut client = match_client(RenderTier::High);
    platform::headless::run(&mut client, SIZE, 30, Some(path)).expect("headless render");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/main-headless.png");
    Golden::new("imagine/viewmodel", combat_plausibility(SIZE))
        .compare(path, &fixture, TOLERANCE)
        .expect("viewmodel golden");
}

#[test]
fn imagine_combat_arena_matches_its_reference_within_taa_noise_bound() {
    let path = scratch("imagine-combat-arena-golden.png");
    let path = path.as_path();
    let mut client = match_client(RenderTier::High);
    platform::headless::run(&mut client, COMBAT_SIZE, 30, Some(path))
        .expect("headless arena render");
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/imagine-combat-feel.png");
    Golden::new("imagine/combat-arena", combat_plausibility(COMBAT_SIZE))
        .compare(path, &fixture, TOLERANCE)
        .expect("combat arena golden");
}

fn match_client(tier: RenderTier) -> ClientApp<ImagineGame, ArenaPresenter> {
    match_client_with_view(tier, ViewMode::Lit)
}

fn match_client_with_view(
    tier: RenderTier,
    view_mode: ViewMode,
) -> ClientApp<ImagineGame, ArenaPresenter> {
    let profile =
        DeviceProfile::detect(DeviceClass::Desktop, None, false).expect("desktop profile");
    let mut settings = tier.settings();
    if matches!(view_mode, ViewMode::OpaqueCoverage { .. }) {
        settings.shadow_size = 128;
        settings.bloom = false;
    }
    settings.view_mode = view_mode;
    let content = content_pack();
    let mut client = ClientApp::new(
        ImagineGame::from_content(&content),
        &ImagineSettings::menu_backdrop(),
        ArenaPresenter::new(&content, false).expect("arena presenter"),
        &app_config(Some(0)).expect("app config"),
        &profile,
        settings,
    )
    .expect("IMAGINE client");
    client
        .mount_content(content)
        .expect("mount IMAGINE content");
    let mut input = Input::default();
    input.screen = MENU_SIZE;
    for _ in 0..90 {
        frame(&mut client, &mut input);
    }
    start_match(&mut client, &mut input);
    client
}

fn frame(client: &mut ClientApp<ImagineGame, ArenaPresenter>, input: &mut Input) {
    client.prepare_input(input).expect("prepare input");
    client.simulate(FRAME_TICK).expect("simulate frame");
    input.end_frame();
}

fn press(client: &mut ClientApp<ImagineGame, ArenaPresenter>, input: &mut Input, key: Code) {
    input.set_code(key, true);
    frame(client, input);
    input.set_code(key, false);
    input.end_frame();
}

/// Clicks a lobby widget by id, so the script does not depend on focus order.
fn click(client: &mut ClientApp<ImagineGame, ArenaPresenter>, input: &mut Input, name: &str) {
    frame(client, input);
    let rect = client
        .presenter()
        .menu_widget_rects(name)
        .first()
        .copied()
        .expect("widget has visible bounds");
    input.mouse_pos = Vec2::new(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5);
    input.set_button(Mouse::Left, true);
    frame(client, input);
    input.set_button(Mouse::Left, false);
    frame(client, input);
}

fn start_match(client: &mut ClientApp<ImagineGame, ArenaPresenter>, input: &mut Input) {
    press(client, input, Code::Enter);
    assert_eq!(client.presenter().flow(), Flow::ModeSelect);
    press(client, input, Code::Enter);
    assert_eq!(client.presenter().flow(), Flow::Lobby);
    for _ in 0..2 {
        click(client, input, "add_bot");
    }
    click(client, input, "ready");
    click(client, input, "start");
    for _ in 0..=COUNTDOWN_TICKS {
        frame(client, input);
        if client.presenter().flow() == Flow::Loading {
            break;
        }
    }
    assert_eq!(client.presenter().flow(), Flow::Loading);
    client.prepare_input(input).expect("prepare match travel");
    assert_eq!(client.presenter().flow(), Flow::Match);
    assert_eq!(
        client
            .app()
            .world()
            .resource::<Actors>()
            .expect("actors")
            .slots()
            .len(),
        3
    );
}

#[test]
fn imagine_combat_floor_is_lit_at_every_quality_tier() {
    for tier in [
        RenderTier::Low,
        RenderTier::Medium,
        RenderTier::High,
        RenderTier::Ultra,
    ] {
        let path = scratch("imagine-combat-plausibility.png");
        let path = path.as_path();
        let mut client = match_client(tier);
        platform::headless::run(&mut client, COMBAT_SIZE, 30, Some(path))
            .expect("headless combat render");
        Golden::new("imagine/combat-floor", combat_plausibility(COMBAT_SIZE))
            .check(path)
            .expect("combat floor plausibility");
        std::fs::remove_file(path).expect("remove rendered combat frame");
    }
}

#[test]
fn imagine_combat_opaque_shading_covers_depth_at_every_tier_and_prepass_state() {
    let path = scratch("imagine-depth-coverage.png");
    let path = path.as_path();
    for tier in [
        RenderTier::Test,
        RenderTier::Low,
        RenderTier::Medium,
        RenderTier::High,
        RenderTier::Ultra,
    ] {
        for prepass in [false, true] {
            let mut client = match_client_with_view(tier, ViewMode::OpaqueCoverage { prepass });
            platform::headless::run(&mut client, (320, 180), 30, Some(path))
                .expect("headless combat coverage render");
            let actual = Image::read(path).expect("coverage render output");
            let (covered, missing_marker) = coverage_counts(&actual.pixels);
            assert!(
                covered > 0,
                "{tier:?}, prepass={prepass}: combat scene must cover pixels"
            );
            assert_eq!(
                missing_marker, 0,
                "{tier:?}, prepass={prepass}: depth-covered pixels without opaque marker"
            );
            std::fs::remove_file(path).expect("remove coverage render");
        }
    }
}

/// Counts depth-covered pixels, and those among them the opaque pass did not mark.
fn coverage_counts(pixels: &[[u8; 4]]) -> (usize, usize) {
    let mut covered = 0usize;
    let mut missing_marker = 0usize;
    for &[marker, depth_covered, _, _] in pixels {
        if depth_covered > 127 {
            covered = covered.saturating_add(1);
            if marker != u8::MAX {
                missing_marker = missing_marker.saturating_add(1);
            }
        }
    }
    (covered, missing_marker)
}

fn combat_plausibility(size: (u32, u32)) -> Plausible {
    let (width, height) = size;
    let floor = ImageRegion::new(
        width * 45 / 100,
        height * 55 / 100,
        width * 55 / 100,
        height * 72 / 100,
    );
    let center = ImageRegion::new(
        width * 45 / 100,
        height * 40 / 100,
        width * 55 / 100,
        height * 60 / 100,
    );
    Plausible::luminance_range("lit floor", floor, 0.03, 0.95).and_not_clear_color(
        "center scene",
        center,
        [0, 0, 0],
        10,
        8,
    )
}
