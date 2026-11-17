//! The HUD. Its layout, text and touch buttons are a UI document (`games/imagine/ui/hud.html`,
//! cooked to `hud.ui.ron`); this module binds the game's numbers to the document's variables each
//! frame and draws the feedback that is geometry, not layout: the crosshair that widens with spread,
//! the hit marker, the kill flash and the damage directions. The frame telemetry panel is drawn by
//! the engine (`app::draw_overlay`).
//!
//! [`HudState::read`] copies everything the HUD needs out of the world once per frame, so drawing
//! reads plain values and never touches the simulation.
//!
//! Invariants: the HUD never writes simulation state; every draw is bounded by the screen; every
//! variable the document names is bound before the first layout (the tests cook the real document
//! and lay it out).

use engine_ui::Bindings;
use engine_world::{World, WorldError};
use glam::Vec2;
use kit_fps::{Health, Weapon};
use render::{Ui, rgba};

use crate::data::{TICK_RATE_HZ, Tuning};
use crate::feedback::{HURT_SLOTS, HudEvents, Hurt};
use crate::rules::{Actors, KILL_FEED_ROWS, KillFeed, Life, PLAYER_SLOT, Score};

/// Logical height the layout is designed at. One logical unit is one pixel at this height.
const LOGICAL_HEIGHT: f32 = 720.0;
/// Crosshair gap with no spread, in logical units.
const CROSSHAIR_BASE_GAP: f32 = 6.0;
/// Extra crosshair gap per degree of spread, in logical units.
const CROSSHAIR_PX_PER_DEG: f32 = 6.0;
/// Crosshair arm length, in logical units.
const CROSSHAIR_ARM: f32 = 9.0;
/// Radius of the damage direction dots, in logical units.
const HURT_DOT_RADIUS: f32 = 14.0;
/// Distance of the damage direction dots from the screen centre, as a fraction of the height.
const HURT_RING_FRACTION: f32 = 0.35;
/// Health at or below this fraction of the maximum turns the health bar red.
const LOW_HEALTH_FRACTION: f32 = 0.25;

/// How many places the score stack shows.
pub const BOARD_ROWS: usize = 3;
/// The build line in the bottom corner.
const BUILD_LINE: &str = concat!("IMAGINE ", env!("CARGO_PKG_VERSION"), " · ARENA");

/// One place in the score stack.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoardRow {
    /// The actor's display name.
    pub name: String,
    /// The actor's kills.
    pub kills: u32,
    /// Whether this is the player.
    pub is_player: bool,
}

/// One line of the kill feed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeedRow {
    /// The killer's display name.
    pub killer: String,
    /// The victim's display name.
    pub victim: String,
    /// Whether the killer is the player.
    pub killer_is_player: bool,
    /// Whether the victim is the player.
    pub victim_is_player: bool,
}

/// What the HUD shows this frame, copied from the world.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HudState {
    /// The player's hit points.
    pub health: u32,
    /// The player's maximum hit points.
    pub max_health: u32,
    /// Ticks until the player respawns, or `None` while the player is alive.
    pub downed_ticks: Option<u32>,
    /// Rounds in the magazine.
    pub ammo: u32,
    /// Magazine capacity.
    pub magazine: u32,
    /// Weapon display name.
    pub rifle_name: String,
    /// Player display name.
    pub player_name: String,
    /// Whether a reload is running.
    pub reloading: bool,
    /// The weapon's current spread half-angle, in radians.
    pub spread_rad: f32,
    /// Whole seconds the match has run.
    pub clock_secs: u32,
    /// The top places by kills, best first; ties go to the lower slot.
    pub board: Vec<BoardRow>,
    /// The latest kills, newest first.
    pub feed: Vec<Option<FeedRow>>,
    /// Who the player downed most recently, while that kill is in the feed (empty otherwise).
    pub eliminated: String,
    /// Ticks the hit marker stays up.
    pub hit_marker_ticks: u32,
    /// Ticks the kill flash stays up.
    pub kill_flash_ticks: u32,
    /// Configured kill-flash lifetime.
    pub kill_flash_lifetime: u32,
    /// Configured damage-indicator lifetime.
    pub damage_indicator_lifetime: u32,
    /// The damage direction indicators.
    pub hurt: [Option<Hurt>; HURT_SLOTS],
}

impl HudState {
    /// Copies the HUD's inputs out of the world.
    ///
    /// # Errors
    ///
    /// Returns a world error when the player, its components or a HUD resource is missing.
    pub fn read(world: &World) -> Result<Self, WorldError> {
        let player = world
            .resource::<Actors>()?
            .get(PLAYER_SLOT)
            .ok_or(WorldError::Invariant("the player actor is missing"))?;
        let health = *world.get::<Health>(player.id())?;
        let weapon = world.get::<Weapon>(player.id())?.state();
        let score = world.resource::<Score>()?;
        let feed = world.resource::<KillFeed>()?;
        let events = world.resource::<HudEvents>()?;
        let tuning = world.resource::<Tuning>()?;
        let downed_ticks = match player.life() {
            Life::Alive => None,
            Life::Down { ticks_left } => Some(ticks_left),
        };
        let mut hurt = [None; HURT_SLOTS];
        for (slot, source) in hurt.iter_mut().zip(events.hurt()) {
            *slot = *source;
        }
        Ok(Self {
            health: health.current(),
            max_health: health.max(),
            downed_ticks,
            ammo: weapon.ammo(),
            magazine: tuning.rifle.magazine,
            rifle_name: tuning.rifle.name.clone(),
            player_name: tuning.actors.player_name.clone(),
            reloading: weapon.reload_left() > 0,
            spread_rad: weapon.spread(),
            clock_secs: score.ticks() / u32::from(TICK_RATE_HZ),
            board: board(score, tuning)?,
            feed: feed_rows(feed, tuning)?,
            eliminated: last_player_kill(feed, tuning)?,
            hit_marker_ticks: events.hit_marker_ticks(),
            kill_flash_ticks: events.kill_flash_ticks(),
            kill_flash_lifetime: tuning.feedback.kill_flash_ticks,
            damage_indicator_lifetime: tuning.feedback.damage_indicator_ticks,
            hurt,
        })
    }
}

/// The display name of the actor in `slot`.
fn name_of(slot: usize, tuning: &Tuning) -> Result<String, WorldError> {
    let name = match slot.checked_sub(1) {
        None => Some(tuning.actors.player_name.clone()),
        Some(bot) => tuning.actors.bot_roster.get(bot).cloned(),
    };
    name.ok_or(WorldError::Invariant("an actor slot has no name"))
}

/// The top places by kills. Sorting a fixed array keeps this allocation-free.
fn board(score: &Score, tuning: &Tuning) -> Result<Vec<BoardRow>, WorldError> {
    let mut places = Vec::with_capacity(score.tallies().len());
    for (slot, tally) in score.tallies().iter().enumerate() {
        places.push(BoardRow {
            name: name_of(slot, tuning)?,
            kills: tally.kills(),
            is_player: slot == PLAYER_SLOT,
        });
    }
    // Stable sort: equal kills keep slot order.
    places.sort_by_key(|place| std::cmp::Reverse(place.kills));
    places.truncate(BOARD_ROWS);
    places.resize_with(BOARD_ROWS, BoardRow::default);
    Ok(places)
}

/// The kill feed's lines with names.
fn feed_rows(feed: &KillFeed, tuning: &Tuning) -> Result<Vec<Option<FeedRow>>, WorldError> {
    let mut rows = Vec::with_capacity(FEED_NAMES.len());
    for entry in feed.rows().iter().take(FEED_NAMES.len()) {
        if let Some(entry) = entry {
            rows.push(Some(FeedRow {
                killer: name_of(entry.killer(), tuning)?,
                victim: name_of(entry.victim(), tuning)?,
                killer_is_player: entry.killer() == PLAYER_SLOT,
                victim_is_player: entry.victim() == PLAYER_SLOT,
            }));
        } else {
            rows.push(None);
        }
    }
    rows.resize(FEED_NAMES.len(), None);
    Ok(rows)
}

/// The victim of the player's newest kill still in the feed, or an empty name.
fn last_player_kill(feed: &KillFeed, tuning: &Tuning) -> Result<String, WorldError> {
    match feed
        .rows()
        .iter()
        .flatten()
        .find(|entry| entry.killer() == PLAYER_SLOT)
    {
        Some(entry) => name_of(entry.victim(), tuning),
        None => Ok(String::new()),
    }
}

/// The screen scale the feedback is drawn at.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    width: f32,
    height: f32,
    unit: f32,
}

impl Layout {
    /// The layout for a surface of `size` pixels.
    fn new(size: Vec2) -> Self {
        Self {
            width: size.x,
            height: size.y,
            unit: size.y / LOGICAL_HEIGHT,
        }
    }

    /// `value` logical units in pixels.
    fn u(self, value: f32) -> f32 {
        value * self.unit
    }
}

/// The control state the document shows: whether the device has touch buttons, whether the scope is
/// on, and which touch buttons a finger holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Controls {
    /// The device has touch input, so the document shows its buttons.
    pub touch: bool,
    /// The scope is on.
    pub scoped: bool,
    /// A finger holds the big fire button.
    pub fire: bool,
    /// A finger holds the left fire button.
    pub fire_left: bool,
    /// The optional left fire button is enabled in settings.
    pub left_fire_setting: bool,
    /// A finger holds the jump button.
    pub jump: bool,
    /// A finger holds the crouch/slide button.
    pub crouch: bool,
    /// A finger holds the reload button.
    pub reload: bool,
    /// Automatic fire is enabled.
    pub auto_fire: bool,
}

/// The variables the HUD document reads, kept between frames so updating them allocates nothing.
#[derive(Debug, Default)]
pub struct HudBindings(Bindings);

/// The document's variables for each score place: name, kills, and whether it is the player.
const BOARD_NAMES: [(&str, &str, &str); BOARD_ROWS] = [
    ("r1_name", "r1_kills", "r1_me"),
    ("r2_name", "r2_kills", "r2_me"),
    ("r3_name", "r3_kills", "r3_me"),
];

/// The document's variables for each feed line: shown, killer, victim, killer is the player, victim
/// is the player.
const FEED_NAMES: [[&str; 5]; KILL_FEED_ROWS] = [
    ["f1_on", "f1_killer", "f1_victim", "f1_kme", "f1_vme"],
    ["f2_on", "f2_killer", "f2_victim", "f2_kme", "f2_vme"],
    ["f3_on", "f3_killer", "f3_victim", "f3_kme", "f3_vme"],
    ["f4_on", "f4_killer", "f4_victim", "f4_kme", "f4_vme"],
];

impl HudBindings {
    /// Current values used by the cooked HUD document.
    #[must_use]
    pub fn bindings(&self) -> &Bindings {
        &self.0
    }

    /// Sets every variable from `state` and `controls` and returns the set.
    pub fn update(&mut self, state: &HudState, controls: Controls) -> &Bindings {
        let b = &mut self.0;
        let health = state.health as f32;
        let max = state.max_health as f32;
        let frac = if state.max_health == 0 {
            0.0
        } else {
            (health / max).clamp(0.0, 1.0)
        };
        let minutes = state.clock_secs / 60;
        let seconds = state.clock_secs % 60;
        b.set_num("clock_m", minutes as f32);
        b.set_num("clock_st", (seconds / 10) as f32);
        b.set_num("clock_so", (seconds % 10) as f32);
        b.set_text("rifle", &state.rifle_name);
        b.set_text("player", &state.player_name);
        b.set_text("build", BUILD_LINE);
        b.set_text("eliminated", &state.eliminated);
        for (index, (name, kills, me)) in BOARD_NAMES.iter().enumerate() {
            let row = state.board.get(index).cloned().unwrap_or_default();
            b.set_text(name, &row.name);
            b.set_num(kills, row.kills as f32);
            b.set_flag(me, row.is_player);
        }
        for ([on, killer, victim, kme, vme], index) in FEED_NAMES.iter().zip(0..KILL_FEED_ROWS) {
            let source = state.feed.get(index).and_then(Option::as_ref);
            b.set_flag(on, source.is_some());
            let row = source.cloned().unwrap_or_default();
            b.set_text(killer, &row.killer);
            b.set_text(victim, &row.victim);
            b.set_flag(kme, row.killer_is_player);
            b.set_flag(vme, row.victim_is_player);
        }
        b.set_num("health", health);
        b.set_num("max_health", max);
        b.set_num("health_frac", frac);
        b.set_flag("low", frac <= LOW_HEALTH_FRACTION);
        b.set_num("ammo", state.ammo as f32);
        b.set_num("mag", state.magazine as f32);
        b.set_flag("reloading", state.reloading);
        b.set_flag("kill", state.kill_flash_ticks > 0);
        b.set_flag("downed", state.downed_ticks.is_some());
        b.set_num("respawn", seconds_of(state.downed_ticks.unwrap_or(0)));
        b.set_flag("touch", controls.touch);
        b.set_flag("auto", controls.auto_fire);
        b.set_flag("scope", controls.scoped);
        b.set_flag("fire", controls.fire);
        b.set_flag("fire_left", controls.fire_left);
        b.set_flag("left_fire_setting", controls.left_fire_setting);
        b.set_flag("h_gear", false);
        b.set_flag("jump", controls.jump);
        b.set_flag("crouch", controls.crouch);
        b.set_flag("reload", controls.reload);
        b
    }
}

/// Draws the feedback that is geometry: the kill flash, the hit marker, the crosshair and the damage
/// directions. The document is drawn after it, on top.
pub fn draw_feedback(ui: &mut Ui, state: &HudState) {
    let layout = Layout::new(ui.size);
    draw_hit_feedback(ui, layout, state);
    draw_crosshair(ui, layout, state);
    draw_hurt(ui, layout, state);
}

/// The crosshair: four arms that move apart as the weapon's spread grows.
fn draw_crosshair(ui: &mut Ui, layout: Layout, state: &HudState) {
    let centre = Vec2::new(layout.width * 0.5, layout.height * 0.5);
    let gap = layout.u(CROSSHAIR_BASE_GAP + state.spread_rad.to_degrees() * CROSSHAIR_PX_PER_DEG);
    let arm = layout.u(CROSSHAIR_ARM);
    let thick = layout.u(2.0);
    let color = rgba(0xffffff, 0.9);
    ui.rect(
        centre.x - thick * 0.5,
        centre.y - gap - arm,
        thick,
        arm,
        0.0,
        color,
    );
    ui.rect(
        centre.x - thick * 0.5,
        centre.y + gap,
        thick,
        arm,
        0.0,
        color,
    );
    ui.rect(
        centre.x - gap - arm,
        centre.y - thick * 0.5,
        arm,
        thick,
        0.0,
        color,
    );
    ui.rect(
        centre.x + gap,
        centre.y - thick * 0.5,
        arm,
        thick,
        0.0,
        color,
    );
}

/// The hit marker (four ticks around the crosshair) and the kill flash (a full-screen wash).
fn draw_hit_feedback(ui: &mut Ui, layout: Layout, state: &HudState) {
    if state.kill_flash_ticks > 0 {
        let fraction = fraction(state.kill_flash_ticks, state.kill_flash_lifetime);
        ui.rect(
            0.0,
            0.0,
            layout.width,
            layout.height,
            0.0,
            rgba(0xffffff, 0.25 * fraction),
        );
    }
    if state.hit_marker_ticks > 0 {
        let centre = Vec2::new(layout.width * 0.5, layout.height * 0.5);
        let inner = layout.u(8.0);
        let outer = layout.u(16.0);
        let width = layout.u(2.0);
        let color = rgba(0xffffff, 1.0);
        for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let a = centre + Vec2::new(sx * inner, sy * inner);
            let b = centre + Vec2::new(sx * outer, sy * outer);
            ui.line(a, b, width, color);
        }
    }
}

/// The damage direction indicators: a dot on the ring around the crosshair, at the shooter's
/// bearing, fading as it expires.
fn draw_hurt(ui: &mut Ui, layout: Layout, state: &HudState) {
    let centre = Vec2::new(layout.width * 0.5, layout.height * 0.5);
    let ring = layout.height * HURT_RING_FRACTION;
    let radius = layout.u(HURT_DOT_RADIUS);
    for hurt in state.hurt.iter().flatten() {
        let alpha = fraction(hurt.ticks_left(), state.damage_indicator_lifetime);
        let bearing = hurt.bearing_rad();
        let at = centre + Vec2::new(bearing.sin(), -bearing.cos()) * ring;
        ui.circle(at.x, at.y, radius, rgba(0xe74c3c, 0.85 * alpha));
    }
}

/// The ticks as seconds at the simulation rate.
fn seconds_of(ticks: u32) -> f32 {
    ticks as f32 / f32::from(TICK_RATE_HZ)
}

/// `part / whole` in `[0, 1]`, and 0 for a zero whole.
fn fraction(part: u32, whole: u32) -> f32 {
    if whole == 0 {
        return 0.0;
    }
    (part as f32 / whole as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use engine_ui::Value;

    fn document() -> engine_ui::Document {
        let content = crate::test_support::pack().expect("test pack");
        let path = engine_assets::AssetPath::new("ui/hud.ui.ron").expect("HUD path");
        engine_ui::Document::from_ron(content.load_text(&path).expect("HUD in pack"))
            .expect("HUD document parses")
    }

    struct ApproxMeasure;

    impl engine_ui::TextMeasure for ApproxMeasure {
        fn metrics_key(&self) -> engine_ui::MetricsKey {
            engine_ui::MetricsKey(1)
        }

        fn width(&mut self, px: f32, text: &str) -> f32 {
            px * 0.55 * f32::from(u16::try_from(text.chars().count()).unwrap_or(u16::MAX))
        }
    }

    #[test]
    fn health_bar_keeps_its_full_track_box_at_full_and_quarter_health() {
        let document = engine_ui::Document::from_ron(include_str!("../../ui/hud.ui.ron"))
            .expect("HUD document parses");
        let mut fill_widths = Vec::new();
        for (health, low) in [(100, false), (25, true)] {
            let state = HudState {
                health,
                max_health: 100,
                player_name: "PLAYER".to_owned(),
                ..HudState::default()
            };
            let mut bindings = HudBindings::default();
            let values = bindings.update(&state, Controls::default()).clone();
            let mut layout = engine_ui::UiLayout::new(document.clone()).expect("HUD layout builds");
            let list = layout
                .frame(&values, (1280.0, 720.0), &mut ApproxMeasure)
                .expect("HUD lays out")
                .clone();
            layout
                .check_visible_content()
                .expect("HUD content has area");
            let rects = layout
                .node_rects()
                .expect("HUD node rectangles are available");
            let bar = rects
                .iter()
                .find(|node| node.visible && node.source.contains("progress.bar"))
                .expect("one health bar is visible");
            assert!(bar.rect.w > 0.0, "bar rectangle: {:?}", bar.rect);
            let wrap = rects
                .iter()
                .find(|node| node.visible && node.source.contains("div.wrap"))
                .expect("health bar wrapper is visible");
            assert_eq!(bar.rect.w, wrap.rect.w);
            let fill = list
                .primitives()
                .iter()
                .find_map(|primitive| match primitive {
                    engine_ui::Primitive::Fill { rect, color, .. }
                        if color.rgb.r == if low { 0xff } else { 0xf3 }
                            && color.rgb.g == if low { 0x3d } else { 0xf5 }
                            && color.rgb.b == if low { 0x52 } else { 0xf8 }
                            && rect.h == bar.rect.h
                            && rect.w > if low { 0.0 } else { 100.0 } =>
                    {
                        Some(rect)
                    }
                    _ => None,
                })
                .expect("bar fill uses the progress rectangle");
            fill_widths.push(fill.w);
        }
        let full = fill_widths.first().expect("full health fill is measured");
        let quarter = fill_widths.get(1).expect("quarter health fill is measured");
        assert!((full * 0.25 - quarter).abs() < 0.001);
    }

    #[test]
    fn hud_text_fits_required_phone_and_desktop_shapes() {
        let document = document();
        let state = HudState {
            player_name: "HALCYON".into(),
            board: (0..BOARD_ROWS)
                .map(|_| BoardRow {
                    name: "HALCYON".into(),
                    kills: 99,
                    is_player: false,
                })
                .collect(),
            feed: (0..KILL_FEED_ROWS)
                .map(|_| {
                    Some(FeedRow {
                        killer: "HALCYON".into(),
                        victim: "ROOK_3".into(),
                        killer_is_player: false,
                        victim_is_player: false,
                    })
                })
                .collect(),
            ..HudState::default()
        };
        let mut bindings = HudBindings::default();
        let values = bindings.update(&state, Controls::default()).clone();
        for size in [
            (1920.0, 1080.0),
            (2400.0, 1080.0),
            (2340.0, 1080.0),
            (1600.0, 720.0),
        ] {
            let mut layout = engine_ui::UiLayout::new(document.clone()).expect("HUD layout builds");
            let list = layout
                .frame(&values, size, &mut ApproxMeasure)
                .expect("HUD lays out");
            let result = engine_ui::check_text_overflow(list);
            assert!(result.is_ok(), "HUD at {size:?}: {result:?}");
        }
    }

    #[test]
    fn hud_matrix_keeps_every_real_interactive_control_inside_all_target_viewports() {
        let document = document();
        let longest = "HALCYON";
        let full_state = HudState {
            health: 100,
            max_health: 100,
            ammo: 30,
            magazine: 30,
            rifle_name: "PRACTICE RIFLE".to_owned(),
            player_name: longest.to_owned(),
            board: (0..BOARD_ROWS)
                .map(|index| BoardRow {
                    name: format!("{longest} {index}"),
                    kills: 999,
                    is_player: index == 0,
                })
                .collect(),
            feed: (0..KILL_FEED_ROWS)
                .map(|index| {
                    Some(FeedRow {
                        killer: format!("{longest} K{index}"),
                        victim: format!("{longest} V{index}"),
                        killer_is_player: index == 0,
                        victim_is_player: index == 1,
                    })
                })
                .collect(),
            ..HudState::default()
        };
        let warnings = HudState {
            health: 1,
            max_health: 100,
            reloading: true,
            downed_ticks: Some(1),
            eliminated: longest.to_owned(),
            hit_marker_ticks: 1,
            kill_flash_ticks: 1,
            kill_flash_lifetime: 1,
            damage_indicator_lifetime: 1,
            ..full_state.clone()
        };
        let cases = [
            (
                "start",
                HudState::default(),
                Controls {
                    touch: true,
                    ..Controls::default()
                },
            ),
            (
                "longest-score-and-full-feed",
                full_state.clone(),
                Controls {
                    touch: true,
                    ..Controls::default()
                },
            ),
            (
                "left-fire-and-ads",
                full_state.clone(),
                Controls {
                    touch: true,
                    left_fire_setting: true,
                    scoped: true,
                    ..Controls::default()
                },
            ),
            (
                "warnings",
                warnings,
                Controls {
                    touch: true,
                    ..Controls::default()
                },
            ),
        ];
        for (width, height) in [
            (1920.0, 1080.0),
            (2400.0, 1080.0),
            (2340.0, 1080.0),
            (2800.0, 1280.0),
            (1600.0, 720.0),
            (640.0, 360.0),
        ] {
            for (name, state, controls) in &cases {
                let mut bindings = HudBindings::default();
                let values = bindings.update(state, *controls).clone();
                let mut layout =
                    engine_ui::UiLayout::new(document.clone()).expect("HUD layout builds");
                let list = layout
                    .frame(&values, (width, height), &mut ApproxMeasure)
                    .expect("HUD lays out")
                    .clone();
                layout
                    .check_visible_content()
                    .expect("HUD content has area");
                let text = engine_ui::check_text_overflow(&list);
                assert!(text.is_ok(), "{name} text at {width}x{height}: {text:?}");
                let interactive = engine_ui::check_interactive_bounds(
                    &list,
                    engine_ui::Rect {
                        x: 0.0,
                        y: 0.0,
                        w: width,
                        h: height,
                    },
                );
                assert!(
                    interactive.is_ok(),
                    "{name} interactive bounds at {width}x{height}: {interactive:?}"
                );
            }
        }
    }

    #[test]
    fn the_board_lists_the_top_kills_first_and_keeps_slot_order_on_ties() {
        let state = HudState {
            board: vec![
                BoardRow {
                    name: "A".to_owned(),
                    kills: 3,
                    is_player: false,
                },
                BoardRow {
                    name: "B".to_owned(),
                    kills: 2,
                    is_player: true,
                },
                BoardRow::default(),
            ],
            ..HudState::default()
        };
        let mut bindings = HudBindings::default();
        let b = bindings.update(&state, Controls::default());
        assert_eq!(b.get("r1_name"), Some(&Value::Text("A".to_owned())));
        assert_eq!(b.get("r2_me"), Some(&Value::Flag(true)));
        assert_eq!(b.get("r2_kills"), Some(&Value::Num(2.0)));
    }

    #[test]
    fn the_clock_splits_into_minutes_and_two_seconds_digits() {
        let state = HudState {
            clock_secs: 10 * 60 + 48,
            ..HudState::default()
        };
        let mut bindings = HudBindings::default();
        let b = bindings.update(&state, Controls::default());
        assert_eq!(b.get("clock_m"), Some(&Value::Num(10.0)));
        assert_eq!(b.get("clock_st"), Some(&Value::Num(4.0)));
        assert_eq!(b.get("clock_so"), Some(&Value::Num(8.0)));
    }

    #[test]
    fn an_empty_feed_row_is_hidden_and_a_full_one_names_both_sides() {
        let state = HudState {
            feed: vec![
                Some(FeedRow {
                    killer: "JUNO".to_owned(),
                    victim: "MARR".to_owned(),
                    killer_is_player: false,
                    victim_is_player: false,
                }),
                None,
                None,
                None,
            ],
            ..HudState::default()
        };
        let mut bindings = HudBindings::default();
        let b = bindings.update(&state, Controls::default());
        assert_eq!(b.get("f1_on"), Some(&Value::Flag(true)));
        assert_eq!(b.get("f1_victim"), Some(&Value::Text("MARR".to_owned())));
        assert_eq!(b.get("f2_on"), Some(&Value::Flag(false)));
    }
}
