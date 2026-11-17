//! A warmed retained menu frame, including repeated focus movement, must not allocate.

use app::GameEntry;
use engine_ui::Layer;
use engine_ui::Value;
use imagine::ImagineEntry;
use imagine::flow::{FlowEffect, Screen};
use imagine::menus::{self, GameMenus};
use platform::{Code, Input};
use stats_alloc::{Region, StatsAlloc};
use std::error::Error;

#[global_allocator]
static GLOBAL: StatsAlloc<std::alloc::System> = StatsAlloc::system();

const FOCUS_FRAMES: usize = 1_000;

#[test]
fn home_menu_focus_frames_allocate_nothing_after_warm_up() -> Result<(), Box<dyn Error>> {
    let stream =
        platform::ContentSource::headless().open_content(ImagineEntry::CONTENT.as_str())?;
    let content = engine_assets::PackReader::read(stream)?;
    let mut menus = GameMenus::new(&content)?;
    menus.apply(FlowEffect::Replace(Screen::Home, Layer::Menu))?;
    let mut input = Input::default();
    input.screen = glam::Vec2::new(1920.0, 1080.0);
    for _ in 0..32 {
        menus.frame(&input, input.screen, 16)?;
        input.set_code(Code::Down, true);
        menus.frame(&input, input.screen, 16)?;
        input.end_frame();
        input.set_code(Code::Down, false);
        input.end_frame();
    }

    let region = Region::new(&GLOBAL);
    for _ in 0..FOCUS_FRAMES / 2 {
        input.set_code(Code::Down, true);
        menus.frame(&input, input.screen, 16)?;
        input.end_frame();
        input.set_code(Code::Down, false);
        input.end_frame();
    }
    assert_eq!(region.change().allocations, 0);

    menus.apply(FlowEffect::Replace(Screen::Lobby, Layer::Menu))?;
    let seat = menus::record(&[
        ("name", Value::Text("Player".into())),
        ("host", Value::Text("♛".into())),
        ("team", Value::Num(1.0)),
        ("kind", Value::Text("PLAYER".into())),
        ("ready", Value::Text("READY".into())),
        ("kick_disabled", Value::Flag(true)),
    ]);
    menus.set_list("seats", menus::list(&[seat]));
    menus.set_text("room_code", "AB12CD");
    menus.set_text("countdown_text", "");
    menus.set_flag("countdown", false);
    menus.set_flag("cannot_start", true);
    for _ in 0..32 {
        menus.frame(&input, input.screen, 16)?;
        input.set_code(Code::Down, true);
        menus.frame(&input, input.screen, 16)?;
        input.end_frame();
        input.set_code(Code::Down, false);
        input.end_frame();
    }
    let lobby_region = Region::new(&GLOBAL);
    for _ in 0..FOCUS_FRAMES / 2 {
        input.set_code(Code::Down, true);
        menus.frame(&input, input.screen, 16)?;
        input.end_frame();
        input.set_code(Code::Down, false);
        input.end_frame();
    }
    assert_eq!(lobby_region.change().allocations, 0);
    Ok(())
}
