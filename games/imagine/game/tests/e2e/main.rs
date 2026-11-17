//! Scripted input drives the real client shell through Results and standalone Leave match.

use std::error::Error;

use app::{ClientApp, GameEntry};
use engine_assets::{AssetPath, ContentPack, PackReader};
use engine_ui::{Document, MetricsKey, TextMeasure, UiLayout};
use glam::Vec2;
use imagine::flow::{Flow, Screen};
use imagine::hud::{Controls, HudBindings, HudState};
use imagine::rules::Actors;
use imagine::rules::Score;
use imagine::session::{COUNTDOWN_TICKS, PRACTICE_MATCH_TICKS};
use imagine::sim::{ImagineGame, ImagineSettings, app_config};
use imagine::{ArenaPresenter, ImagineEntry};
use platform::{Code, DeviceClass, DeviceProfile, Input, Mouse};
use render::Settings;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const FRAME_TICK: f32 = 1.0 / 60.0;
const MENU_SIZE: Vec2 = Vec2::new(1920.0, 1080.0);

fn client() -> TestResult<ClientApp<ImagineGame, ArenaPresenter>> {
    Ok(app::headless::start::<ImagineEntry>()?)
}

fn client_for(
    class: DeviceClass,
    touch: bool,
) -> TestResult<ClientApp<ImagineGame, ArenaPresenter>> {
    let profile = DeviceProfile::detect(class, None, touch)?;
    let content = content_pack()?;
    let mut client = ClientApp::new(
        ImagineGame::from_content(&content),
        &ImagineSettings::menu_backdrop(),
        ArenaPresenter::new(&content, touch)?,
        &app_config(Some(0))?,
        &profile,
        Settings::default(),
    )?;
    client.mount_content(content)?;
    Ok(client)
}

fn content_pack() -> TestResult<ContentPack> {
    let stream =
        platform::ContentSource::headless().open_content(ImagineEntry::CONTENT.as_str())?;
    Ok(PackReader::read(stream)?)
}

fn assert_viewmodel_mesh_resident(content: &ContentPack) -> TestResult {
    let pool = jobs::JobPool::new(0)?;
    let path = AssetPath::new("content/cooked/weapons/pistol.meshpack")?;
    let mut store = engine_assets::AssetStore::new();
    store.mount_content(content.clone(), &pool)?;
    let handle = store.load::<engine_assets::MeshPack>(&pool, path)?;
    store.apply_loaded(&pool, std::time::Instant::now())?;
    match store.status(&handle)? {
        engine_assets::AssetStatus::Ready(mesh) if !mesh.meshes.is_empty() => Ok(()),
        status => Err(format!("viewmodel mesh is not resident: {status:?}").into()),
    }
}

fn frame(client: &mut ClientApp<ImagineGame, ArenaPresenter>, input: &mut Input) -> TestResult {
    client.prepare_input(input)?;
    client.simulate(FRAME_TICK)?;
    input.end_frame();
    assert!(
        client.is_running(),
        "the client stopped during a scripted frame"
    );
    assert!(
        client.error_banner_message().is_none(),
        "the client showed an error banner"
    );
    client.presenter_mut().check_text_overflow(input.screen)?;
    Ok(())
}

fn press(
    client: &mut ClientApp<ImagineGame, ArenaPresenter>,
    input: &mut Input,
    key: Code,
) -> TestResult {
    input.set_code(key, true);
    frame(client, input)?;
    input.set_code(key, false);
    input.end_frame();
    Ok(())
}

fn initialized_client() -> TestResult<(ClientApp<ImagineGame, ArenaPresenter>, Input)> {
    let mut client = client()?;
    let mut input = Input::default();
    input.screen = MENU_SIZE;
    for _ in 0..90 {
        frame(&mut client, &mut input)?;
    }
    assert_eq!(client.presenter().flow(), Flow::Home);
    Ok((client, input))
}

fn tap_widget(
    client: &mut ClientApp<ImagineGame, ArenaPresenter>,
    input: &mut Input,
    name: &str,
    index: usize,
) -> TestResult {
    frame(client, input)?;
    let rect = client
        .presenter()
        .menu_widget_rects(name)
        .get(index)
        .copied()
        .ok_or_else(|| {
            format!(
                "{name}[{index}] has no visible bounds on {:?}",
                client.presenter().top_menu_screen()
            )
        })?;
    input.touch_start(1, Vec2::new(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5));
    frame(client, input)?;
    input.touch_end(1);
    frame(client, input)
}

fn click_widget(
    client: &mut ClientApp<ImagineGame, ArenaPresenter>,
    input: &mut Input,
    name: &str,
    index: usize,
) -> TestResult {
    frame(client, input)?;
    let rect = client
        .presenter()
        .menu_widget_rects(name)
        .get(index)
        .copied()
        .ok_or_else(|| format!("{name}[{index}] has no visible bounds"))?;
    input.mouse_pos = Vec2::new(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5);
    input.set_button(Mouse::Left, true);
    frame(client, input)?;
    input.set_button(Mouse::Left, false);
    frame(client, input)
}

struct TestMeasure;

impl TextMeasure for TestMeasure {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(0)
    }

    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * 0.5 * text.chars().count() as f32
    }
}

/// Installs HUD regions from the same cooked document and binding path as `UiScreen::draw`.
/// The headless e2e has no GPU font atlas, so `TestMeasure` replaces only glyph measurement; touch
/// coordinates always come from these installed regions and never from a fallback coordinate.
fn install_drawn_hud_regions(input: &mut Input) -> TestResult {
    let content = content_pack()?;
    let document = Document::from_ron(content.load_text(&AssetPath::new("ui/hud.ui.ron")?)?)?;
    let mut bindings = HudBindings::default();
    let values = bindings.update(
        &HudState::default(),
        Controls {
            touch: true,
            ..Controls::default()
        },
    );
    let mut layout = UiLayout::new(document)?;
    let list = layout.frame(values, (input.screen.x, input.screen.y), &mut TestMeasure)?;
    if !input.set_touch_regions(list.touch_regions()) {
        return Err("drawn HUD regions were invalid".into());
    }
    Ok(())
}

fn touch_button_position(input: &Input, id: platform::TouchRegionId) -> TestResult<Vec2> {
    let region = input
        .touch_regions()
        .iter()
        .find(|region| region.id == id)
        .ok_or_else(|| format!("touch region {} was not installed by the HUD", id.0))?;
    let rect = region.rect.rect();
    Ok(Vec2::new(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5))
}

fn pause_settings_resume(
    client: &mut ClientApp<ImagineGame, ArenaPresenter>,
    input: &mut Input,
) -> TestResult {
    press(client, input, Code::Esc)?;
    assert_eq!(
        client.presenter().top_menu_screen(),
        Some(Screen::MatchMenu)
    );
    click_widget(client, input, "settings", 0)?;
    click_widget(client, input, "close", 0)?;
    assert_eq!(
        client.presenter().top_menu_screen(),
        Some(Screen::MatchMenu)
    );
    click_widget(client, input, "resume", 0)?;
    assert_eq!(client.presenter().flow(), Flow::Match);
    Ok(())
}

#[test]
fn phone_touch_runs_a_full_capacity_room_through_results() -> TestResult {
    let mut client = client_for(DeviceClass::Handheld, true)?;
    let mut input = Input::default();
    input.screen = Vec2::new(2800.0, 1280.0);
    for _ in 0..90 {
        frame(&mut client, &mut input)?;
    }
    assert_eq!(client.presenter().flow(), Flow::Home);
    tap_widget(&mut client, &mut input, "play", 0)?;
    assert_eq!(client.presenter().flow(), Flow::ModeSelect);
    tap_widget(&mut client, &mut input, "mode_select", 2)?;
    assert_eq!(client.presenter().flow(), Flow::Lobby);
    for _ in 0..6 {
        tap_widget(&mut client, &mut input, "add_bot", 0)?;
    }
    tap_widget(&mut client, &mut input, "remove_bot", 0)?;
    tap_widget(&mut client, &mut input, "ready", 0)?;
    tap_widget(&mut client, &mut input, "start", 0)?;
    assert_eq!(client.presenter().flow(), Flow::Lobby);
    for _ in 0..=COUNTDOWN_TICKS {
        frame(&mut client, &mut input)?;
        if client.presenter().flow() == Flow::Loading {
            break;
        }
    }
    assert_eq!(client.presenter().flow(), Flow::Loading);
    client.prepare_input(&mut input)?;
    assert_eq!(client.presenter().flow(), Flow::Match);
    assert_eq!(client.app().world().resource::<Actors>()?.slots().len(), 6);
    let content = content_pack()?;
    assert_viewmodel_mesh_resident(&content)?;
    frame(&mut client, &mut input)?;
    pause_settings_resume(&mut client, &mut input)?;
    install_drawn_hud_regions(&mut input)?;

    let fire_center = touch_button_position(&input, imagine::touch::FIRE)?;
    input.touch_start(1, fire_center);
    input.touch_start(2, Vec2::new(120.0, input.screen.y * 0.5));
    let mut temporary_touches = Vec::with_capacity(4);
    for tick in 0..PRACTICE_MATCH_TICKS {
        temporary_touches.clear();
        if tick % 120 == 0 {
            input.touch_start(3, touch_button_position(&input, imagine::touch::RELOAD)?);
            temporary_touches.push(3);
        }
        if tick % 60 == 0 {
            input.touch_start(4, touch_button_position(&input, imagine::touch::JUMP)?);
            temporary_touches.push(4);
        }
        if tick == 0 {
            input.touch_start(5, touch_button_position(&input, imagine::touch::SCOPE)?);
            input.touch_start(6, touch_button_position(&input, imagine::touch::CROUCH)?);
            temporary_touches.extend([5, 6]);
        }
        frame(&mut client, &mut input)?;
        for touch in &temporary_touches {
            input.touch_end(*touch);
        }
    }
    input.touch_end(1);
    input.touch_end(2);
    assert_eq!(client.presenter().flow(), Flow::Results);
    tap_widget(&mut client, &mut input, "continue", 0)?;
    assert_eq!(client.presenter().flow(), Flow::Home);
    Ok(())
}

fn enter_lobby_and_add_bots(
    client: &mut ClientApp<ImagineGame, ArenaPresenter>,
    input: &mut Input,
) -> TestResult {
    click_widget(client, input, "play", 0)?;
    assert_eq!(client.presenter().flow(), Flow::ModeSelect);
    click_widget(client, input, "mode_select", 2)?;
    assert_eq!(client.presenter().flow(), Flow::Lobby);

    for _ in 0..6 {
        click_widget(client, input, "add_bot", 0)?;
    }
    click_widget(client, input, "remove_bot", 0)?;
    click_widget(client, input, "ready", 0)?;
    click_widget(client, input, "start", 0)?;
    Ok(())
}

fn start_match(
    client: &mut ClientApp<ImagineGame, ArenaPresenter>,
    input: &mut Input,
) -> TestResult {
    let travel_epoch = client.app().travel_epoch();
    enter_lobby_and_add_bots(client, input)?;
    for _ in 0..=COUNTDOWN_TICKS {
        frame(client, input)?;
        if client.presenter().flow() == Flow::Loading {
            break;
        }
    }
    assert_eq!(client.presenter().flow(), Flow::Loading);
    assert_eq!(client.app().travel_epoch(), travel_epoch);
    client.prepare_input(input)?;
    assert_eq!(client.app().travel_epoch(), travel_epoch + 1);
    assert_eq!(client.presenter().flow(), Flow::Match);
    assert_eq!(client.app().world().resource::<Actors>()?.slots().len(), 6);
    pause_settings_resume(client, input)?;
    Ok(())
}

fn results_run() -> TestResult<u64> {
    let (mut client, mut input) = initialized_client()?;
    let initial_epoch = client.app().travel_epoch();
    assert_eq!(client.presenter().flow(), Flow::Home);
    for completed_matches in 0..2 {
        start_match(&mut client, &mut input)?;
        input.set_code(Code::W, true);
        input.set_button(Mouse::Left, true);
        for tick in 0..PRACTICE_MATCH_TICKS {
            input.set_code(Code::Space, tick % 60 == 0);
            input.set_code(Code::R, tick % 120 == 0);
            input.set_code(Code::Q, tick == 0);
            input.set_code(Code::C, tick == 0);
            frame(&mut client, &mut input)?;
        }
        input.set_code(Code::W, false);
        input.set_button(Mouse::Left, false);
        assert_eq!(client.presenter().flow(), Flow::Results);
        assert_eq!(client.presenter().top_menu_screen(), Some(Screen::Results));
        let summary = client
            .presenter()
            .match_summary()
            .ok_or("finished match summary is missing")?;
        let actors = client.app().world().resource::<Actors>()?;
        let score = client.app().world().resource::<Score>()?;
        assert_eq!(summary.actors.len(), actors.slots().len());
        for (slot, _) in actors.slots().iter().enumerate() {
            let row = summary.actors.get(slot).ok_or("summary row is missing")?;
            let tally = score.tally(slot).ok_or("rules tally is missing")?;
            assert_eq!(row.kills, tally.kills());
            assert_eq!(row.deaths, tally.deaths());
        }
        press(&mut client, &mut input, Code::Enter)?;
        assert_eq!(client.presenter().flow(), Flow::Home);
        assert_eq!(
            client.app().travel_epoch(),
            initial_epoch + 2 * (completed_matches + 1)
        );
    }
    Ok(client.app().world().state_hash()?)
}

#[test]
fn headless_scripts_visit_expected_screens_and_travel_twice_deterministically() -> TestResult {
    let first = results_run()?;
    let second = results_run()?;
    assert_eq!(first, second);

    let (mut client, mut input) = initialized_client()?;
    start_match(&mut client, &mut input)?;
    press(&mut client, &mut input, Code::Backspace)?;
    assert_eq!(client.presenter().flow(), Flow::Match);
    assert_eq!(
        client.presenter().top_menu_screen(),
        Some(Screen::MatchMenu)
    );
    for _ in 0..3 {
        press(&mut client, &mut input, Code::Down)?;
    }
    press(&mut client, &mut input, Code::Enter)?;
    assert_eq!(
        client.presenter().top_menu_screen(),
        Some(Screen::LeaveConfirm)
    );
    press(&mut client, &mut input, Code::Right)?;
    press(&mut client, &mut input, Code::Enter)?;
    assert_eq!(client.presenter().flow(), Flow::Home);
    assert_eq!(client.app().travel_epoch(), 2);
    Ok(())
}
