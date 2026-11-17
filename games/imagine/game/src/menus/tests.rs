//! IMAGINE menu screen behavior tests.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use super::{GameMenus, list, record};
use crate::data::{load_tuning, mode_definitions};
use crate::flow::Screen;
use crate::results::{ActorResult, MatchSummary};
use engine_core::StateHasher;
use engine_ui::{Document, Layer, Value};
use glam::Vec2;
use std::io::Cursor;

use engine_assets::{AssetPath, ContentPack, PackReader};

fn menu_content() -> ContentPack {
    let sources = [
        ("ui/theme.ron", include_str!("../../../ui/theme.ron")),
        ("ui/boot.ui.ron", include_str!("../../../ui/boot.ui.ron")),
        ("ui/home.ui.ron", include_str!("../../../ui/home.ui.ron")),
        ("ui/modes.ui.ron", include_str!("../../../ui/modes.ui.ron")),
        ("ui/lobby.ui.ron", include_str!("../../../ui/lobby.ui.ron")),
        (
            "ui/loading.ui.ron",
            include_str!("../../../ui/loading.ui.ron"),
        ),
        (
            "ui/match_menu.ui.ron",
            include_str!("../../../ui/match_menu.ui.ron"),
        ),
        (
            "ui/scoreboard.ui.ron",
            include_str!("../../../ui/scoreboard.ui.ron"),
        ),
        (
            "ui/results.ui.ron",
            include_str!("../../../ui/results.ui.ron"),
        ),
        (
            "ui/settings.ui.ron",
            include_str!("../../../ui/settings.ui.ron"),
        ),
        (
            "ui/loadout.ui.ron",
            include_str!("../../../ui/loadout.ui.ron"),
        ),
        (
            "ui/confirm.ui.ron",
            include_str!("../../../ui/confirm.ui.ron"),
        ),
        ("data/modes.ron", include_str!("../../../data/modes.ron")),
        ("data/tuning.ron", include_str!("../../../data/tuning.ron")),
    ];
    let entries = sources
        .into_iter()
        .map(|(path, text)| {
            (
                AssetPath::new(path).expect("valid test asset path"),
                text.as_bytes().to_vec(),
            )
        })
        .collect::<Vec<_>>();
    let mut bytes = Vec::new();
    ContentPack::write(entries, &mut bytes).expect("encode menu test content");
    PackReader::read(Cursor::new(bytes)).expect("read menu test content")
}

#[test]
fn every_cooked_menu_document_validates() {
    for screen in [
        Screen::Boot,
        Screen::Home,
        Screen::Modes,
        Screen::Lobby,
        Screen::Loading,
        Screen::MatchMenu,
        Screen::Scoreboard,
        Screen::Results,
        Screen::Settings,
        Screen::Loadout,
        Screen::LeaveConfirm,
    ] {
        let source = super::documents::document_source(&menu_content(), screen)
            .expect("menu source is packed");
        Document::from_ron(&source).expect("cooked screen validates");
    }
    GameMenus::new(&menu_content()).expect("menu runtime loads startup data");
}

#[test]
fn a_rejected_menu_reload_keeps_the_last_good_screen() {
    let mut menus = GameMenus::new(&menu_content()).expect("menu runtime loads startup data");
    let original = menus.document_source(Screen::Boot).to_owned();
    assert!(
        menus
            .replace_document_source(Screen::Boot, "not a document".to_owned())
            .is_err()
    );
    assert_eq!(menus.document_source(Screen::Boot), original);
    assert_eq!(menus.active_screen(Layer::Menu), Some(Screen::Boot));
}

#[test]
fn a_loading_screen_pushed_during_a_tick_is_laid_out_for_that_draw() {
    let mut menus = GameMenus::new(&menu_content()).expect("menu runtime loads startup data");
    menus
        .apply(crate::flow::FlowEffect::Replace(
            Screen::Loading,
            Layer::Menu,
        ))
        .expect("loading screen opens");
    assert_eq!(menus.runtime.draw_list().count(), 0);
    menus
        .layout_for_draw(Vec2::new(1920.0, 1080.0))
        .expect("loading screen lays out before drawing");
    assert_eq!(menus.runtime.draw_list().count(), 1);
}

#[test]
fn every_menu_screen_fits_the_longest_game_strings_at_phone_and_desktop_sizes() {
    let sizes = [
        Vec2::new(1920.0, 1080.0),
        Vec2::new(2400.0, 1080.0),
        Vec2::new(2340.0, 1080.0),
        Vec2::new(1600.0, 720.0),
    ];
    let content = menu_content();
    let mode_path = AssetPath::new("data/modes.ron").expect("valid modes path");
    let modes = mode_definitions(content.load_text(&mode_path).expect("mode data"))
        .expect("mode data validates");
    let tuning = load_tuning(&content).expect("embedded tuning validates");
    let names = std::iter::once(tuning.actors.player_name.clone())
        .chain(tuning.actors.bot_roster.iter().cloned())
        .take(3)
        .collect::<Vec<_>>();
    let seats = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            record(&[
                ("name", Value::Text(name.clone())),
                ("team", Value::Num((index % 2) as f32)),
                ("ready", Value::Text("READY".into())),
                (
                    "kind",
                    Value::Text(if index == 0 { "PLAYER" } else { "BOT" }.into()),
                ),
                (
                    "host",
                    Value::Text(if index == 0 { "♛" } else { "" }.into()),
                ),
                ("kick_disabled", Value::Flag(index == 0)),
            ])
        })
        .collect::<Vec<_>>();
    let scores = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            record(&[
                ("name", Value::Text(name.clone())),
                ("team", Value::Num((index % 2) as f32)),
                ("kills", Value::Num(10.0)),
                ("deaths", Value::Num(3.0)),
            ])
        })
        .collect::<Vec<_>>();

    for screen in [
        Screen::Boot,
        Screen::Home,
        Screen::Modes,
        Screen::Lobby,
        Screen::Loading,
        Screen::MatchMenu,
        Screen::Scoreboard,
        Screen::Results,
        Screen::Settings,
        Screen::Loadout,
        Screen::LeaveConfirm,
    ] {
        for size in sizes {
            let mut menus = GameMenus::new(&content).expect("menu runtime loads");
            menus
                .apply(crate::flow::FlowEffect::Replace(screen, Layer::Menu))
                .expect("screen opens");
            menus.set_text(
                "mode",
                modes
                    .iter()
                    .map(|mode| mode.name.as_str())
                    .max_by_key(|name| name.len())
                    .unwrap_or("PRACTICE"),
            );
            menus.set_text("arena", "KILN YARD");
            menus.set_text("room_code", "AB12CD");
            menus.set_list("seats", list(&seats));
            menus.set_num("seat_count", names.len() as f32);
            menus.set_num("seat_capacity", names.len() as f32);
            menus.set_flag("cannot_start", false);
            menus.set_list("scores", list(&scores));
            menus.set_text("placement", "VICTORY");
            menus.set_text("winner", "TEAM 1");
            menus.set_num("kills", 99.0);
            menus.set_num("deaths", 3.0);
            menus.set_num("score", 9999.0);
            for (tab, flags) in [
                (0, [true, false, false, false]),
                (1, [false, true, false, false]),
                (2, [false, false, true, false]),
                (3, [false, false, false, true]),
            ] {
                menus.set_num("tab_index", tab as f32);
                menus.set_flag("graphics", flags[0]);
                menus.set_flag("controls", flags[1]);
                menus.set_flag("audio", flags[2]);
                menus.set_flag("accessibility", flags[3]);
                let input = platform::Input::default();
                for _ in 0..12 {
                    menus.frame(&input, size, 16).expect("screen lays out");
                }
                menus
                    .runtime
                    .check_visible_content()
                    .expect("visible menu content has area");
                for draw in menus.runtime.draw_list() {
                    let result = engine_ui::check_text_overflow(draw.list);
                    assert!(
                        result.is_ok(),
                        "{screen:?} at {size:?}: {result:?}; runs: {:?}",
                        draw.list.text_runs()
                    );
                }
            }
        }
    }
}

fn draw_hash(screen: Screen, size: Vec2) -> u64 {
    let content = menu_content();
    let mut menus = GameMenus::new(&content).expect("menu runtime loads startup data");
    menus
        .apply(crate::flow::FlowEffect::Replace(screen, Layer::Menu))
        .expect("golden screen opens");
    if screen == Screen::Lobby {
        let seats = [
            record(&[
                ("name", Value::Text("PLAYER".into())),
                ("team", Value::Num(1.0)),
                ("ready", Value::Text("READY".into())),
                ("kind", Value::Text("PLAYER".into())),
                ("host", Value::Text("♛".into())),
                ("kick_disabled", Value::Flag(true)),
            ]),
            record(&[
                ("name", Value::Text("BOT 01".into())),
                ("team", Value::Num(2.0)),
                ("ready", Value::Text("READY".into())),
                ("kind", Value::Text("BOT".into())),
                ("host", Value::Text("".into())),
                ("kick_disabled", Value::Flag(false)),
            ]),
        ];
        menus.set_list("seats", list(&seats));
        menus.set_text("room_code", "AB12CD");
        menus.set_num("seat_count", 2.0);
    }
    if screen == Screen::Results {
        let summary = MatchSummary {
            actors: vec![
                ActorResult {
                    name: "PLAYER".into(),
                    team: 0,
                    kills: 8,
                    deaths: 2,
                },
                ActorResult {
                    name: "BOT 01".into(),
                    team: 1,
                    kills: 4,
                    deaths: 3,
                },
            ],
            winner_team: Some(0),
        };
        menus.set_results(&summary, 8);
    }
    let input = platform::Input::default();
    for _ in 0..12 {
        menus
            .frame(&input, size, 16)
            .expect("golden screen lays out");
    }
    let mut hasher = StateHasher::new();
    for draw in menus.runtime.draw_list() {
        hasher.write_bytes(format!("{:?}", draw.layer).as_bytes());
        hasher.write_bytes(format!("{:?}", draw.list.primitives()).as_bytes());
        hasher.write_bytes(draw.list.text().as_bytes());
    }
    hasher.finish()
}

#[test]
fn home_lobby_and_results_draw_lists_match_desktop_and_phone_goldens() {
    let sizes = [Vec2::new(1920.0, 1080.0), Vec2::new(2400.0, 1080.0)];
    let screens = [Screen::Home, Screen::Lobby, Screen::Results];
    let actual = screens
        .into_iter()
        .flat_map(|screen| sizes.map(|size| draw_hash(screen, size)))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            0x95b4_f3ba_11b0_5677,
            0xb35a_c2ea_6489_97a6,
            0xcf26_d36f_2e60_d3bc,
            0x9d05_fba1_8de0_44fc,
            0x46e0_1540_d9e2_b8a5,
            0x3c4a_1467_d83d_0189,
        ]
    );
}

#[test]
fn phone_draw_lists_match_goldens_for_every_menu_screen() {
    let screens = [
        Screen::Boot,
        Screen::Home,
        Screen::Modes,
        Screen::Lobby,
        Screen::Loading,
        Screen::MatchMenu,
        Screen::Scoreboard,
        Screen::Results,
        Screen::Settings,
        Screen::Loadout,
        Screen::LeaveConfirm,
    ];
    let actual = screens
        .into_iter()
        .map(|screen| draw_hash(screen, Vec2::new(2400.0, 1080.0)))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            0x97cc_970e_c9a7_eb01,
            0xb35a_c2ea_6489_97a6,
            0xda80_d498_a8fb_2c94,
            0x9d05_fba1_8de0_44fc,
            0xc11b_9146_de82_8e02,
            0x901f_e708_ca4b_2736,
            0xf0a5_8bf6_f516_2505,
            0x3c4a_1467_d83d_0189,
            0x0abc_8709_10cc_43a9,
            0x5c89_71cd_4aeb_c93f,
            0x6c80_1b5b_4ac2_3473,
        ]
    );
}
