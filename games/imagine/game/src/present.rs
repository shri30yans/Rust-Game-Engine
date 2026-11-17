//! The arena presenter: keyboard, mouse and touch in; the arena, the bots, the tracers, a
//! first-person rifle and the HUD out.
//!
//! Controls: WASD (or arrows) move, Space jumps, the left mouse button fires, R reloads, F3 toggles
//! the frame telemetry panel (D45); the mouse looks. Touch: the left stick moves, a right-half drag
//! looks, and the on-screen buttons fire, aim (a drag on the big FIRE button steers too), scope,
//! jump and reload. The buttons are part of the HUD document (`games/imagine/ui/hud.html`), shown
//! only when the presenter is built with touch controls (Android); the touch areas come from the
//! same layout pass as the picture. On touch the telemetry panel starts shown, and an invisible
//! button over its corner toggles it.
//!
//! The presenter reads the world once per frame in [`Presenter::apply_input`] and draws only from
//! that copy, so drawing never touches the simulation. Bots are drawn as a capsule and a head
//! sphere, tinted by team, and blended between the last two ticks.
//!
//! Invariants: a downed bot or player is not drawn; a tracer is drawn only while it is young; the
//! viewmodel is drawn only while the player is alive.

use std::collections::HashMap;
use std::time::Instant;

use app::{
    AppError, Audio, Bus, ClientAssets, ErrorBanner, FrameReport, LookSensitivity, Presenter,
    RenderFrame, ScenePresenter, SettingsFile, TELEMETRY_TOUCH_ID, UiScreen, UploadStatus,
    ViewAngles, Volume, blend, draw_overlay,
};
use engine_assets::{AssetPath, ContentPack, Handle, MeshPack};
use engine_ui::{Layer as MenuLayer, Value as MenuValue};
use engine_world::{NetId, World, WorldError};
use glam::{Affine3A, Mat4, Quat, Vec2, Vec3};
use kit_fps::{AimAssist, AimAssistParams, AimTarget, MoveCommand, Team, Trigger};
use platform::{Code, Input, Mouse, TouchRegion};
use render::{
    Camera, FontId, Gfx, GpuTimes, Layer, Look as RenderLook, Mat, MeshBuilder, MeshId,
    PrimitiveDesc, PrimitiveId, srgb,
};
use thiserror::Error;

use crate::audio::ImagineAudio;
use crate::data::{
    AimAssistTuning, CAPSULE_HALF_HEIGHT_M, CAPSULE_RADIUS_M, EYE_ABOVE_CENTRE_M, Tuning,
    load_tuning, mode_definitions,
};
use crate::feedback::{HudEvents, Tracer};
use crate::flow::{Flow, FlowAction, FlowEffect, Screen};
use crate::hud::{self, Controls, HudBindings, HudState};
use crate::menus::{self, GameMenus, MenuAction, SettingControl};
use crate::results::MatchSummary;
use crate::rules::{Actors, PLAYER_SLOT, Score, Tally};
use crate::scene::{BoxRole, Scene};
use crate::session::LocalSession;
use crate::settings::{FireMode, GraphicsRevert, QualityTier, Settings};
use crate::sim::{apply_tuning, is_alive};
use crate::touch;

/// Radius of a bot's head sphere, in metres.
/// How much the scope narrows the view, and how much it slows the look, so a pixel of drag turns
/// the picture by the same amount on screen.
const SCOPE_FOV_SCALE: f32 = 0.5;
/// Presentation-only camera kick per shot.
const CAMERA_RECOIL_KICK_RAD: f32 = 0.012;
/// Recoil recovery duration at 60 Hz, about 120 ms.
const CAMERA_RECOIL_TICKS: u32 = 7;
/// Touch sprint begins after this many consecutive ticks above 90% stick deflection.
const TOUCH_SPRINT_HOLD_TICKS: u16 = 15;
/// Radius used for a target's aim-assist sphere.
const AIM_TARGET_RADIUS_M: f32 = 0.9;
/// Aim target centre above the actor origin.
const AIM_TARGET_CENTRE_ABOVE_M: f32 = 0.35;
const RESULT_COUNTUP_TICKS: u32 = 60;

/// Errors building the arena presenter and its preloaded sound bank.
#[derive(Debug, Error)]
pub enum PresenterError {
    /// The HUD, settings, or engine-facing presenter setup failed.
    #[error(transparent)]
    App(#[from] AppError),
    /// The game sound configuration or one of its sound assets failed validation/loading.
    #[error(transparent)]
    AudioConfig(#[from] crate::audio::SoundConfigError),
}

impl From<PresenterError> for AppError {
    fn from(error: PresenterError) -> Self {
        match error {
            PresenterError::App(error) => error,
            PresenterError::AudioConfig(error) => Self::AudioConfig(error.to_string()),
        }
    }
}

const HEAD_RADIUS_M: f32 = 0.16;
/// Height of a bot's head centre above its capsule centre, in metres.
const HEAD_OFFSET_M: f32 = 0.92;
/// Thickness of a tracer, in metres.
const TRACER_WIDTH_M: f32 = 0.025;
/// Emissive strength of a tracer.
const TRACER_EMISSIVE: f32 = 4.0;
/// Colour of a tracer (linear RGB).
const TRACER_COLOUR: [f32; 3] = [1.0, 0.85, 0.5];
/// Reach of a muzzle flash light, in metres.
const MUZZLE_LIGHT_RADIUS_M: f32 = 4.0;
/// Intensity of a muzzle flash light.
const MUZZLE_LIGHT_INTENSITY: f32 = 3.0;
/// Colour of a muzzle flash light (linear RGB).
const MUZZLE_LIGHT_COLOUR: [f32; 3] = [1.0, 0.7, 0.35];
/// Draws the arena and turns input into the player's command.
pub struct ArenaPresenter {
    cooked_scene: ScenePresenter,
    cube: Option<MeshId>,
    capsule: Option<MeshId>,
    head: Option<MeshId>,
    pistol: Option<Handle<MeshPack>>,
    pistol_bronze: Option<Handle<MeshPack>>,
    pistol_crimson: Option<Handle<MeshPack>>,
    view_angles: ViewAngles,
    font: Option<FontId>,
    /// Each scene box's role, sorted by entity, loaded from [`Scene`] on the first frame.
    roles: Vec<(NetId, BoxRole)>,
    /// Every actor's entity in slot order, loaded on the first frame.
    actor_ids: Vec<NetId>,
    /// Each actor's team, in slot order, read every frame.
    actor_teams: Vec<u8>,
    /// Reused enemy targets for touch aim slowdown.
    aim_targets: Vec<AimTarget>,
    /// Source values for detecting a newly loaded aim-assist tuning group.
    aim_assist_tuning: Option<AimAssistTuning>,
    /// Validated aim-assist values built once when tuning loads.
    aim_assist_params: Option<AimAssistParams>,
    /// Look direction used to prepare the next frame's touch slowdown.
    latest_look: ViewAngles,
    /// Aim slowdown prepared from the latest rendered frame.
    aim_scale: f32,
    /// Each actor's liveness, in slot order, read every frame.
    alive: Vec<bool>,
    /// The latest tracer of each actor slot, copied every frame.
    tracers: Vec<Option<Tracer>>,
    draws: HashMap<DrawKey, DrawState>,
    stale_draws: Vec<DrawKey>,
    draw_frame: u64,
    muzzle_flash_ticks: u32,
    hud: Option<HudState>,
    /// Whether the telemetry panel is drawn. F3 toggles it; on touch it starts on.
    show_telemetry: bool,
    /// The engine's latest frame telemetry, handed over by `observe` once per frame.
    telemetry: FrameReport,
    /// The render scale in effect, handed over by `observe`.
    render_scale: f32,
    /// The latest GPU timing, when the adapter reports one, handed over by `observe`.
    gpu: Option<GpuTimes>,
    /// Declared present rate for this surface session, from `observe`.
    presentation: platform::PresentationBinding,
    /// Whether the device has touch input, so the touch buttons are drawn.
    show_touch_buttons: bool,
    /// Aiming down the sights: a narrower view and a slower look, toggled by the scope button or Q.
    scoped: bool,
    /// Desktop and touch crouch toggle, represented as a level in `MoveCommand`.
    crouch_latched: bool,
    /// Consecutive touch ticks above the sprint threshold.
    touch_sprint_ticks: u16,
    /// Latest camera eye offset copied from the player's locomotion.
    camera_eye_above_centre: f32,
    /// Whether the latest player movement mode is sliding.
    sliding: bool,
    /// The HUD document (`games/imagine/ui/hud.html`, cooked). In a desktop debug build it reloads
    /// when its file changes.
    hud_ui: UiScreen,
    hud_hash: u64,
    tuning_hash: u64,
    scene_hash: u64,
    sound_hash: u64,
    active_arena_id: u16,
    /// The document's variables, kept between frames.
    hud_bindings: HudBindings,
    /// The control state the document shows this frame, copied from the input.
    controls: Controls,
    /// Where the settings persist; `None` in tests that build a presenter without a store.
    settings_file: Option<SettingsFile<Settings>>,
    /// The last rejected `tuning.ron` reload, shown while the last good tuning stays in force.
    tuning_error: ErrorBanner,
    /// The level watcher and its keep-last-good error banner.
    content: ContentPack,
    /// The player's settings and the menu state.
    settings: Settings,
    audio: ImagineAudio,
    /// The look speed `settings` give, refreshed whenever they change.
    sensitivity: LookSensitivity,
    menus: GameMenus,
    flow: Flow,
    local_session: Option<LocalSession>,
    pending_travel: Option<crate::sim::ImagineSettings>,
    boot_ticks: u32,
    match_ended: bool,
    graphics_revert: GraphicsRevert,
    match_summary: Option<MatchSummary>,
    score_cache: Vec<Tally>,
    result_countup_ticks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DrawKey {
    Entity(NetId, u8),
    Tracer(NetId),
    ViewModel(u8),
}

#[derive(Clone, Copy)]
struct DrawState {
    primitive: PrimitiveId,
    mesh: MeshId,
    look: RenderLook,
    transform_bits: [u32; 16],
    frame_seen: u64,
}

fn load_hud(content: &ContentPack) -> Result<UiScreen, AppError> {
    let path = AssetPath::new("ui/hud.ui.ron")?;
    UiScreen::from_source(content.load_text(&path)?)
}

impl ArenaPresenter {
    /// Current major phase, exposed for game-shell integration and headless flow verification.
    #[must_use]
    pub const fn flow(&self) -> Flow {
        self.flow
    }

    /// Currently topmost menu screen, if a menu layer is active.
    #[must_use]
    pub fn top_menu_screen(&self) -> Option<Screen> {
        [MenuLayer::Modal, MenuLayer::GameMenu, MenuLayer::Menu]
            .into_iter()
            .find_map(|layer| self.menus.active_screen(layer))
    }

    /// Checks visible menus and, in a match, the HUD against their measured text bounds.
    ///
    /// # Errors
    ///
    /// Returns a UI error if text extends beyond its box or parent.
    pub fn check_text_overflow(&mut self, screen: Vec2) -> Result<(), AppError> {
        self.menus.check_text_overflow()?;
        if self.flow == Flow::Match
            && let Some(hud) = self.hud.as_ref()
        {
            let bindings = self.hud_bindings.update(hud, self.controls);
            self.hud_ui
                .check_text_overflow(bindings, (screen.x, screen.y))?;
        }
        Ok(())
    }

    /// Bounds of the named widgets on the active menu, in physical screen pixels.
    #[must_use]
    pub fn menu_widget_rects(&self, name: &str) -> Vec<engine_ui::Rect> {
        self.menus.widget_rects(name)
    }

    /// Final match stats copied before the results screen replaces the finished world.
    #[must_use]
    pub const fn match_summary(&self) -> Option<&MatchSummary> {
        self.match_summary.as_ref()
    }

    /// A presenter with the HUD document. The touch buttons are shown and active only when
    /// `show_touch_buttons` is set. The telemetry panel starts shown on touch and hidden on desktop.
    ///
    /// # Errors
    ///
    /// Returns [`PresenterError::AudioConfig`] when sound data fails validation or decoding, or
    /// [`PresenterError::App`] when the HUD or settings setup fails.
    pub fn new(content: &ContentPack, show_touch_buttons: bool) -> Result<Self, PresenterError> {
        Self::build(content, show_touch_buttons, Settings::default(), None)
    }

    /// Builds a presenter whose settings load from and save to the engine settings file.
    pub fn with_settings_file(
        content: &ContentPack,
        show_touch_buttons: bool,
        mut settings_file: SettingsFile<Settings>,
    ) -> Result<Self, PresenterError> {
        let (settings, warning) = settings_file.load();
        if let Some(warning) = warning {
            platform::log_error(&format!("imagine settings: {warning}"));
        }
        Self::build(content, show_touch_buttons, settings, Some(settings_file))
    }

    fn build(
        content: &ContentPack,
        show_touch_buttons: bool,
        settings: Settings,
        settings_file: Option<SettingsFile<Settings>>,
    ) -> Result<Self, PresenterError> {
        let hud_hash = content
            .hash(&AssetPath::new("ui/hud.ui.ron").map_err(AppError::from)?)
            .map_err(AppError::from)?;
        let tuning_hash = content
            .hash(&AssetPath::new("data/tuning.ron").map_err(AppError::from)?)
            .map_err(AppError::from)?;
        let scene_hash = content
            .hash(&AssetPath::new("data/arenas/kiln_yard.level.ron").map_err(AppError::from)?)
            .map_err(AppError::from)?;
        let sound_hash = ImagineAudio::content_hash(content)?;
        Ok(Self {
            content: content.clone(),
            cooked_scene: ScenePresenter::default(),
            cube: None,
            capsule: None,
            head: None,
            pistol: None,
            pistol_bronze: None,
            pistol_crimson: None,
            view_angles: ViewAngles::ZERO,
            font: None,
            roles: Vec::new(),
            actor_ids: Vec::new(),
            actor_teams: Vec::new(),
            aim_targets: Vec::with_capacity(16),
            aim_assist_tuning: None,
            aim_assist_params: None,
            latest_look: ViewAngles::ZERO,
            aim_scale: 1.0,
            alive: Vec::new(),
            tracers: Vec::new(),
            draws: HashMap::new(),
            stale_draws: Vec::new(),
            draw_frame: 0,
            muzzle_flash_ticks: 0,
            hud: None,
            show_telemetry: show_touch_buttons,
            telemetry: FrameReport::default(),
            render_scale: 0.0,
            gpu: None,
            presentation: platform::PresentationBinding::default(),
            show_touch_buttons,
            scoped: false,
            crouch_latched: false,
            touch_sprint_ticks: 0,
            camera_eye_above_centre: EYE_ABOVE_CENTRE_M,
            sliding: false,
            hud_ui: load_hud(content)?,
            hud_hash,
            tuning_hash,
            scene_hash,
            sound_hash,
            active_arena_id: 0,
            hud_bindings: HudBindings::default(),
            controls: Controls::default(),
            settings_file,
            tuning_error: ErrorBanner::default(),
            settings,
            audio: ImagineAudio::new(content)?,
            sensitivity: settings.look_sensitivity()?,
            menus: GameMenus::new(content)?,
            flow: Flow::Boot,
            local_session: None,
            pending_travel: None,
            boot_ticks: 0,
            match_ended: false,
            graphics_revert: GraphicsRevert::default(),
            match_summary: None,
            score_cache: Vec::new(),
            result_countup_ticks: 0,
        })
    }

    /// Flips the scope on Q or on a press of the scope button.
    fn update_scope(&mut self, input: &Input) {
        if input.hit(Code::Q) || input.touch_region_pressed(touch::SCOPE) {
            self.scoped = !self.scoped;
        }
    }

    /// Flips the telemetry panel on F3 or on a press of the telemetry button.
    fn update_telemetry_toggle(&mut self, input: &Input) {
        if input.hit(Code::F3) || input.touch_region_pressed(TELEMETRY_TOUCH_ID) {
            self.show_telemetry = !self.show_telemetry;
        }
    }

    /// Applies desktop/touch toggle input and emits the held movement levels for this tick.
    fn movement_command(&mut self, command: MoveCommand, input: &Input) -> MoveCommand {
        if input.hit(Code::C) || input.hit(Code::Ctrl) || input.touch_region_pressed(touch::CROUCH)
        {
            self.crouch_latched = !self.crouch_latched;
        }
        if self.show_touch_buttons && input.move_axis().length_squared() > 0.9 * 0.9 {
            self.touch_sprint_ticks = self.touch_sprint_ticks.saturating_add(1);
        } else {
            self.touch_sprint_ticks = 0;
        }
        let sprint = input.down(Code::Shift)
            || (self.show_touch_buttons && self.touch_sprint_ticks >= TOUCH_SPRINT_HOLD_TICKS);
        command.with_movement(sprint, self.crouch_latched)
    }

    /// Applies device-neutral menu input and routes screen actions to the flow or local room.
    fn update_menu(&mut self, input: &Input) -> Result<(), AppError> {
        if self.flow == Flow::Match
            && self.menus.active_screen(MenuLayer::GameMenu).is_none()
            && self.menus.active_screen(MenuLayer::Modal).is_none()
            && (input.hit(Code::Esc)
                || input.hit(Code::Backspace)
                || input.touch_region_pressed(touch::GEAR))
        {
            let effect = self.flow.transition(FlowAction::Pause);
            self.menus.apply(effect)?;
            return Ok(());
        }
        self.menus.frame(input, input.screen, 16)?;
        for index in 0..self.menus.action_count() {
            if let Some(action) = self.menus.action(index) {
                self.handle_menu_action(action)?;
            }
        }
        Ok(())
    }

    fn handle_menu_action(&mut self, action: MenuAction) -> Result<(), AppError> {
        let effect = match action {
            MenuAction::Play => self.flow.transition(FlowAction::Play),
            MenuAction::SelectMode(index) => {
                let modes_path = AssetPath::new("data/modes.ron")?;
                let modes = mode_definitions(self.content.load_text(&modes_path)?)
                    .map_err(|error| AppError::Ui(engine_ui::UiError::Parse(error)))?;
                let Some(mode) = modes.get(index).filter(|mode| mode.available) else {
                    return Ok(());
                };
                if mode.id != "practice" {
                    return Ok(());
                }
                let tuning = load_tuning(&self.content)?;
                match LocalSession::new(
                    crate::sim::ImagineSettings::default(),
                    0x1a_6a_91,
                    mode,
                    &tuning.actors.bot_roster,
                ) {
                    Ok(session) => {
                        self.local_session = Some(session);
                        self.flow.transition(FlowAction::ModeChosen)
                    }
                    Err(error) => {
                        self.menus.set_text("toast", session_error_label(&error));
                        FlowEffect::None
                    }
                }
            }
            MenuAction::AddBot => {
                self.room_action(|room| room.add_bot().map(|_| ()))?;
                FlowEffect::None
            }
            MenuAction::RemoveBot => {
                if let Some(session) = &mut self.local_session
                    && let Err(error) =
                        session.remove_bot_at(session.view().seats.len().saturating_sub(1))
                {
                    self.menus.set_text("toast", session_error_label(&error));
                }
                FlowEffect::None
            }
            MenuAction::ChangeTeam(row) => {
                if let Some(session) = &mut self.local_session
                    && let Err(error) = session.cycle_team_at(row)
                {
                    self.menus.set_text("toast", session_error_label(&error));
                }
                FlowEffect::None
            }
            MenuAction::Kick(row) => {
                if let Some(session) = &mut self.local_session
                    && let Err(error) = session.remove_bot_at(row)
                {
                    self.menus.set_text("toast", session_error_label(&error));
                }
                FlowEffect::None
            }
            MenuAction::Ready => {
                self.room_action(LocalSession::toggle_ready)?;
                FlowEffect::None
            }
            MenuAction::Start => {
                self.room_action(LocalSession::start)?;
                FlowEffect::None
            }
            MenuAction::Resume => self.flow.transition(FlowAction::Resume),
            MenuAction::Scoreboard => self.flow.transition(FlowAction::Scoreboard),
            MenuAction::CloseScoreboard => self.flow.transition(FlowAction::CloseScoreboard),
            MenuAction::LeaveMatch => {
                self.menus.set_text("title", "LEAVE MATCH?");
                self.menus
                    .set_text("body", "YOU WILL RETURN TO THE HOME SCREEN.");
                self.flow.transition(FlowAction::LeaveMatch)
            }
            MenuAction::Confirm => {
                self.pending_travel = Some(crate::sim::ImagineSettings::menu_backdrop());
                self.local_session = None;
                self.flow.transition(FlowAction::ConfirmLeave)
            }
            MenuAction::Cancel => {
                let layer = if self.menus.active_screen(MenuLayer::Modal).is_some() {
                    MenuLayer::Modal
                } else {
                    MenuLayer::Menu
                };
                FlowEffect::Pop(layer)
            }
            MenuAction::Back => {
                if self.menus.active_screen(MenuLayer::Modal).is_some()
                    || self.menus.active_screen(MenuLayer::Menu) == Some(Screen::Settings)
                    || self.menus.active_screen(MenuLayer::Menu) == Some(Screen::Loadout)
                {
                    FlowEffect::None
                } else {
                    if self.flow == Flow::Lobby {
                        self.pending_travel = Some(crate::sim::ImagineSettings::menu_backdrop());
                        self.local_session = None;
                    }
                    self.flow.transition(FlowAction::Back)
                }
            }
            MenuAction::Continue => {
                if let Some(session) = &mut self.local_session
                    && session.phase() == engine_sessions::Phase::Results
                    && let Err(error) = session.return_to_lobby()
                {
                    self.menus.set_text("toast", &error.to_string());
                    return Ok(());
                }
                self.local_session = None;
                self.pending_travel = Some(crate::sim::ImagineSettings::menu_backdrop());
                self.flow.transition(FlowAction::Continue)
            }
            MenuAction::Settings => {
                self.open_settings()?;
                FlowEffect::Push(Screen::Settings, MenuLayer::Menu)
            }
            MenuAction::Loadout => FlowEffect::Push(Screen::Loadout, MenuLayer::Menu),
            MenuAction::SettingChanged(control, value) => {
                self.change_setting(control, value)?;
                FlowEffect::None
            }
            MenuAction::QualityTier(index) => {
                if let Some(quality) = QualityTier::from_index(index) {
                    self.graphics_revert.begin(self.settings.graphics());
                    self.settings.set_quality(quality);
                    self.menus.set_num("quality_index", f32::from(index));
                    self.menus.set_text(
                        "quality",
                        ["LOW", "MEDIUM", "HIGH", "ULTRA"]
                            .get(usize::from(index))
                            .copied()
                            .unwrap_or("MEDIUM"),
                    );
                }
                FlowEffect::None
            }
            MenuAction::SettingToggled(SettingControl::ReduceMotion, value) => {
                self.settings.set_reduce_motion(value);
                self.menus.set_reduce_motion(value);
                self.save_settings();
                FlowEffect::None
            }
            MenuAction::SettingToggled(SettingControl::AimAssist, value) => {
                self.settings.set_aim_assist(value);
                self.menus.set_flag("aim_assist", value);
                self.save_settings();
                FlowEffect::None
            }
            MenuAction::SettingToggled(SettingControl::LeftFire, value) => {
                self.settings.set_fire_left(value);
                self.menus.set_flag("left_fire", value);
                self.save_settings();
                FlowEffect::None
            }
            MenuAction::SettingToggled(_, _) => FlowEffect::None,
            MenuAction::FireMode => {
                self.settings.toggle_mode();
                self.menus
                    .set_text("fire_mode", self.settings.mode().label());
                self.save_settings();
                FlowEffect::None
            }
            MenuAction::SettingsTab(index) => {
                self.menus.set_num("tab_index", f32::from(index));
                for (tab, name) in ["graphics", "controls", "audio", "accessibility"]
                    .into_iter()
                    .enumerate()
                {
                    self.menus.set_flag(name, tab == usize::from(index));
                }
                FlowEffect::None
            }
            MenuAction::ConfirmGraphics => {
                self.graphics_revert.confirm();
                self.save_settings();
                FlowEffect::None
            }
        };
        self.menus.apply(effect)?;
        if self.flow == Flow::Lobby {
            self.bind_menu_data()?;
        }
        Ok(())
    }

    fn room_action(
        &mut self,
        action: impl FnOnce(&mut LocalSession) -> Result<(), crate::session::LocalSessionError>,
    ) -> Result<(), AppError> {
        if let Some(session) = &mut self.local_session
            && let Err(error) = action(session)
        {
            self.menus.set_text("toast", session_error_label(&error));
        }
        Ok(())
    }

    fn open_settings(&mut self) -> Result<(), AppError> {
        self.menus.set_num("tab_index", 0.0);
        self.menus.set_num(
            "quality_index",
            f32::from(self.settings.graphics().quality.index()),
        );
        self.menus.set_flag("graphics", true);
        self.menus.set_flag("controls", false);
        self.menus.set_flag("audio", false);
        self.menus.set_flag("accessibility", false);
        self.menus
            .set_num("sens_x", f32::from(self.settings.sens_x()));
        self.menus
            .set_num("sens_y", f32::from(self.settings.sens_y()));
        self.menus
            .set_num("volume", f32::from(self.settings.volume()));
        self.menus
            .set_num("music_volume", f32::from(self.settings.music_volume()));
        self.menus
            .set_num("sfx_volume", f32::from(self.settings.sfx_volume()));
        self.menus
            .set_num("voice_volume", f32::from(self.settings.voice_volume()));
        self.menus
            .set_num("fov", f32::from(self.settings.graphics().fov));
        self.menus.set_num(
            "render_scale",
            f32::from(self.settings.graphics().render_scale) / 100.0,
        );
        self.menus
            .set_flag("reduce_motion", self.settings.reduce_motion());
        self.menus
            .set_flag("aim_assist", self.settings.aim_assist());
        self.menus.set_flag("left_fire", self.settings.fire_left());
        self.menus
            .set_text("fire_mode", self.settings.mode().label());
        Ok(())
    }

    fn change_setting(&mut self, control: SettingControl, value: f32) -> Result<(), AppError> {
        let value = if control == SettingControl::RenderScale {
            value * 100.0
        } else {
            value
        };
        let is_graphics = matches!(
            control,
            SettingControl::RenderScale | SettingControl::FieldOfView
        );
        let original = self.settings.graphics();
        if self.settings.set_range(control, value) {
            self.sensitivity = self.settings.look_sensitivity()?;
            if is_graphics {
                self.graphics_revert.begin(original);
            } else {
                self.save_settings();
            }
        }
        Ok(())
    }

    fn save_settings(&mut self) {
        if let Some(settings_file) = &mut self.settings_file
            && let Err(error) = settings_file.save_if_changed(&self.settings)
        {
            platform::log_error(&format!("imagine settings could not be saved: {error}"));
        }
    }

    fn bind_menu_data(&mut self) -> Result<(), AppError> {
        if let Some(session) = &self.local_session
            && self.flow == Flow::Lobby
        {
            let view = session.view();
            let records = view
                .seats
                .iter()
                .map(|seat| {
                    let (kind, can_kick) = match seat.occupant {
                        engine_sessions::Occupant::Human { .. } => ("PLAYER", false),
                        engine_sessions::Occupant::Bot => ("BOT", true),
                    };
                    menus::record(&[
                        ("name", MenuValue::Text(seat.name.as_str().to_owned())),
                        ("team", MenuValue::Num(f32::from(seat.team.index()) + 1.0)),
                        (
                            "ready",
                            MenuValue::Text(if seat.ready { "READY" } else { "NOT READY" }.into()),
                        ),
                        (
                            "host",
                            MenuValue::Text(
                                if view.host == Some(seat.id) {
                                    "♛"
                                } else {
                                    ""
                                }
                                .into(),
                            ),
                        ),
                        ("kind", MenuValue::Text(kind.into())),
                        ("kick_disabled", MenuValue::Flag(!can_kick)),
                    ])
                })
                .collect::<Vec<_>>();
            self.menus.set_list("seats", menus::list(&records));
            self.menus
                .set_text("room_code", &session.code().to_string());
            self.menus.set_num(
                "seat_count",
                u8::try_from(view.seats.len()).map_or(0.0, f32::from),
            );
            self.menus
                .set_num("seat_capacity", f32::from(session.max_players()));
            let ready = view
                .seats
                .iter()
                .find(|seat| view.host == Some(seat.id))
                .is_some_and(|seat| seat.ready);
            self.menus
                .set_text("ready_label", if ready { "UNREADY" } else { "READY" });
            self.menus.set_flag(
                "cannot_start",
                !ready
                    || view.seats.len() < usize::from(session.min_players())
                    || session.phase() != engine_sessions::Phase::Lobby,
            );
            self.menus
                .set_flag("bot_capacity_reached", !session.can_add_bot());
            self.menus
                .set_flag("no_bots_to_remove", !session.can_remove_bot());
            let countdown = matches!(session.phase(), engine_sessions::Phase::Countdown { .. });
            self.menus.set_flag("countdown", countdown);
            self.menus.set_text(
                "countdown_text",
                if countdown { "MATCH STARTING" } else { "" },
            );
        }
        Ok(())
    }

    fn refresh_scoreboard(&mut self, world: &World) -> Result<(), AppError> {
        let score = world.resource::<Score>()?;
        if score.tallies() == self.score_cache {
            return Ok(());
        }
        self.score_cache.clear();
        self.score_cache.extend_from_slice(score.tallies());
        let actors = world.resource::<Actors>()?;
        let mut records = Vec::with_capacity(actors.slots().len());
        for (slot, actor) in actors.slots().iter().enumerate() {
            let tally = score
                .tally(slot)
                .ok_or(WorldError::Invariant("score row is missing"))?;
            let team = world.get::<Team>(actor.id())?.number();
            records.push(menus::record(&[
                (
                    "name",
                    MenuValue::Text(if slot == PLAYER_SLOT {
                        "PLAYER".to_owned()
                    } else {
                        format!("BOT {slot:02}")
                    }),
                ),
                ("team", MenuValue::Text(team.to_string())),
                ("kills", MenuValue::Text(tally.kills().to_string())),
                ("deaths", MenuValue::Text(tally.deaths().to_string())),
            ]));
        }
        self.menus.set_list("scores", menus::list(&records));
        Ok(())
    }

    /// Copies which touch buttons a finger holds, and the scope state, for the document to show.
    fn snapshot_controls(&mut self, input: &Input) {
        self.controls = Controls {
            touch: self.show_touch_buttons,
            scoped: self.scoped,
            fire: input.touch_region_held(touch::FIRE),
            fire_left: input.touch_region_held(touch::FIRE_LEFT),
            left_fire_setting: self.settings.fire_left(),
            jump: input.touch_region_held(touch::JUMP),
            crouch: input.touch_region_held(touch::CROUCH),
            reload: input.touch_region_held(touch::RELOAD),
            auto_fire: self.settings.mode() == FireMode::Auto,
        };
    }

    /// The scene role of `id`, or `None` for an entity that is not a scene box.
    fn role_of(&self, id: NetId) -> Option<BoxRole> {
        match self.roles.binary_search_by_key(&id, |(known, _)| *known) {
            Ok(index) => self.roles.get(index).map(|(_, role)| *role),
            Err(_) => None,
        }
    }

    /// The actor slot of `id`, or `None` when the entity is not an actor.
    fn slot_of(&self, id: NetId) -> Option<usize> {
        self.actor_ids.iter().position(|known| *known == id)
    }

    /// Updates cached render roles after a level replacement, reusing the existing allocation.
    fn refresh_scene_roles(&mut self, world: &World) -> Result<(), AppError> {
        let entries = world.resource::<Scene>()?.entries();
        let changed = self.roles.len() != entries.len()
            || self
                .roles
                .iter()
                .zip(entries)
                .any(|((id, role), entry)| *id != entry.id || *role != entry.role);
        if changed {
            self.roles.clear();
            self.roles
                .extend(entries.iter().map(|entry| (entry.id, entry.role)));
            self.roles.sort_by_key(|(id, _)| *id);
        }
        Ok(())
    }

    /// Loads the scene roles and the actor entities. Runs once, on the first frame.
    fn load_identities(&mut self, world: &World) -> Result<(), AppError> {
        if self.roles.is_empty() {
            self.roles = world
                .resource::<Scene>()?
                .entries()
                .iter()
                .map(|entry| (entry.id, entry.role))
                .collect();
            self.roles.sort_by_key(|(id, _)| *id);
        }
        if self.actor_ids.is_empty() {
            self.actor_ids = world
                .resource::<Actors>()?
                .slots()
                .iter()
                .map(|actor| actor.id())
                .collect();
            self.actor_teams.resize(self.actor_ids.len(), 0);
            self.alive.resize(self.actor_ids.len(), false);
            self.tracers.resize(self.actor_ids.len(), None);
        }
        Ok(())
    }

    /// Copies every per-frame value the drawing needs out of the world.
    fn snapshot(&mut self, world: &World) -> Result<(), AppError> {
        for (slot, id) in self.actor_ids.iter().enumerate() {
            let team = self
                .actor_teams
                .get_mut(slot)
                .ok_or(WorldError::Invariant("actor slot is out of range"))?;
            *team = world.get::<Team>(*id)?.number();
            let alive = self
                .alive
                .get_mut(slot)
                .ok_or(WorldError::Invariant("actor slot is out of range"))?;
            *alive = is_alive(world, slot)?;
        }
        let events = world.resource::<HudEvents>()?;
        self.muzzle_flash_ticks = world.resource::<Tuning>()?.feedback.muzzle_flash_ticks;
        for (dst, src) in self.tracers.iter_mut().zip(events.tracers()) {
            *dst = *src;
        }
        self.hud = Some(HudState::read(world)?);
        if let Some(player) = world.resource::<Actors>()?.get(PLAYER_SLOT) {
            let locomotion = world.get::<kit_fps::Locomotion>(player.id())?;
            self.camera_eye_above_centre = locomotion.eye_above_centre();
            self.sliding = matches!(locomotion.move_state(), kit_fps::MoveState::Slide { .. });
        }
        Ok(())
    }

    /// The player's move, jump, fire and reload for this frame, from the input.
    /// Automatic fire is a server-resolved trigger intent, never a client world query.
    fn command(
        look: ViewAngles,
        input: &Input,
        fire_mode: FireMode,
    ) -> Result<MoveCommand, AppError> {
        let keys = Vec2::new(
            axis(input.down(Code::D), input.down(Code::A)),
            axis(input.down(Code::W), input.down(Code::S)),
        );
        let walk = (input.move_axis() + keys).clamp_length_max(1.0);
        let jump_pressed = input.hit(Code::Space) || input.touch_region_pressed(touch::JUMP);
        let jump_held = input.down(Code::Space);
        // The engine binds Space to the fire key, so the keyboard fire key is ignored while Space
        // is held: Space jumps and never fires. The FIRE touch button is not affected by Space.
        let held = input.button(Mouse::Left)
            || (input.fire() && !input.down(Code::Space))
            || input.touch_region_held(touch::FIRE)
            || input.touch_region_held(touch::FIRE_LEFT);
        let trigger = if held {
            Trigger::Held
        } else if fire_mode == FireMode::Auto {
            Trigger::OnTarget
        } else {
            Trigger::Released
        };
        let reload = input.hit(Code::R) || input.touch_region_pressed(touch::RELOAD);
        Ok(kit_fps::error::lift(MoveCommand::new(
            walk,
            look,
            jump_pressed,
            jump_held,
            trigger,
            reload,
        ))?)
    }
}

/// The signed value of a pair of keys: +1, -1, or 0 when both or neither are held.
fn axis(positive: bool, negative: bool) -> f32 {
    match (positive, negative) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    }
}

/// The tint of a scene box, by role. Linear RGB from sRGB hex.
fn box_tint(role: BoxRole) -> [f32; 3] {
    match role {
        BoxRole::Floor | BoxRole::Wall => srgb(0x7d_8590),
        BoxRole::Crate => srgb(0xe0_7b39),
        BoxRole::Building => srgb(0xc9_c2b0),
        BoxRole::Ramp => srgb(0xb8_a98a),
        BoxRole::Platform => srgb(0x9a_a5b1),
    }
}

/// The tint of an actor's body: blue for the player's team (0), red for everyone else, since in a
/// free for all every other actor is an enemy.
fn team_tint(team: u8) -> [f32; 3] {
    match team {
        0 => srgb(0x0034_98db),
        _ => srgb(0x00e7_4c3c),
    }
}

fn team_accent(team: u8) -> [f32; 3] {
    match team {
        0 => [0.05, 0.75, 1.0],
        _ => [1.0, 0.12, 0.015],
    }
}

impl ArenaPresenter {
    fn presentation_recoil(&self) -> f32 {
        self.tracers
            .get(PLAYER_SLOT)
            .copied()
            .flatten()
            .filter(|tracer| tracer.age() < CAMERA_RECOIL_TICKS)
            .map(|tracer| {
                let age = u8::try_from(tracer.age()).unwrap_or(u8::MAX);
                let duration = u8::try_from(CAMERA_RECOIL_TICKS).unwrap_or(u8::MAX);
                CAMERA_RECOIL_KICK_RAD * (1.0 - f32::from(age) / f32::from(duration))
            })
            .unwrap_or(0.0)
    }

    fn prepare_aim_scale(&mut self, frame: &RenderFrame, alpha: f32) -> Result<(), AppError> {
        self.aim_targets.clear();
        self.aim_scale = 1.0;
        if !self.show_touch_buttons || !self.settings.aim_assist() || self.flow != Flow::Match {
            return Ok(());
        }
        let Some(params) = self.aim_assist_params else {
            return Ok(());
        };
        let Some(player_id) = self.actor_ids.first().copied() else {
            return Ok(());
        };
        let Some(player) = frame.blended(player_id, alpha)? else {
            return Ok(());
        };
        let Some(player_team) = self.actor_teams.first().copied() else {
            return Ok(());
        };
        for slot in 1..self.actor_ids.len() {
            if !self.alive.get(slot).copied().unwrap_or(false)
                || self.actor_teams.get(slot).copied() == Some(player_team)
            {
                continue;
            }
            let Some(id) = self.actor_ids.get(slot).copied() else {
                continue;
            };
            let Some(transform) = frame.blended(id, alpha)? else {
                continue;
            };
            self.aim_targets.push(AimTarget {
                centre: transform.translation() + Vec3::Y * AIM_TARGET_CENTRE_ABOVE_M,
                radius: AIM_TARGET_RADIUS_M,
            });
        }
        self.aim_scale = AimAssist::look_scale(
            player.translation() + Vec3::Y * self.camera_eye_above_centre,
            kit_fps::look_direction(self.latest_look),
            &self.aim_targets,
            params,
        );
        Ok(())
    }
}

impl Presenter for ArenaPresenter {
    type Command = MoveCommand;
    type Settings = crate::sim::ImagineSettings;

    fn setup(&mut self, gfx: &mut Gfx, assets: &mut ClientAssets<'_>) -> Result<(), AppError> {
        self.draws.clear();
        self.stale_draws.clear();
        self.draw_frame = 0;
        self.cooked_scene.setup(gfx)?;
        self.pistol = Some(assets.load(AssetPath::new("content/cooked/weapons/pistol.meshpack")?)?);
        self.pistol_bronze = Some(assets.load(AssetPath::new(
            "content/cooked/weapons/pistol_bronze.meshpack",
        )?)?);
        self.pistol_crimson = Some(assets.load(AssetPath::new(
            "content/cooked/weapons/pistol_crimson.meshpack",
        )?)?);
        let surface = Mat::new([1.0; 3], 0.8, 0.0);
        self.cube = Some(gfx.add_mesh(&MeshBuilder::unit_cube(surface))?);
        let mut capsule = MeshBuilder::new();
        capsule.capsule(
            Vec3::ZERO,
            CAPSULE_RADIUS_M,
            CAPSULE_HALF_HEIGHT_M,
            16,
            Mat::new([1.0; 3], 0.7, 0.0),
        );
        self.capsule = Some(gfx.add_mesh(&capsule)?);
        let mut head = MeshBuilder::new();
        head.sphere(
            Vec3::ZERO,
            Vec3::splat(HEAD_RADIUS_M),
            12,
            8,
            Mat::new([1.0; 3], 0.6, 0.0),
        );
        self.head = Some(gfx.add_mesh(&head)?);
        self.draws.reserve(4096);
        self.stale_draws.reserve(4096);
        let font_path = AssetPath::new("fonts/Roboto-Medium.ttf")?;
        let font = gfx.ui.atlas.add_font(assets.content_bytes(&font_path)?)?;
        self.font = Some(font);
        Ok(())
    }

    fn look_scale(&self) -> f32 {
        (if self.scoped { SCOPE_FOV_SCALE } else { 1.0 }) * self.aim_scale
    }

    fn look_sensitivity(&self) -> Option<LookSensitivity> {
        Some(self.sensitivity)
    }

    fn grab_mouse(&self) -> bool {
        self.flow == Flow::Match
            && self.menus.active_screen(MenuLayer::GameMenu).is_none()
            && self.menus.active_screen(MenuLayer::Modal).is_none()
    }

    fn before_ticks(
        &mut self,
        world: &mut World,
        assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        self.content = assets.content_pack().clone();
        let sound_hash = ImagineAudio::content_hash(assets.content_pack())
            .map_err(|error| AppError::AudioConfig(error.to_string()))?;
        if sound_hash != self.sound_hash {
            match ImagineAudio::new(&self.content) {
                Ok(audio) => {
                    self.audio = audio;
                    self.sound_hash = sound_hash;
                }
                Err(error) => self.tuning_error.show(error.to_string(), Instant::now()),
            }
        }
        if let Some(original) = self.graphics_revert.tick() {
            self.settings.set_graphics(original);
        }
        if self.flow == Flow::Boot {
            self.boot_ticks = self.boot_ticks.saturating_add(1);
            if self.boot_ticks >= 90 {
                let effect = self.flow.transition(FlowAction::BootComplete);
                self.menus.apply(effect)?;
            }
        }
        let mut refresh_lobby = false;
        if let Some(session) = &mut self.local_session {
            let update = session.tick();
            if let Some(error) = &update.failure {
                self.menus.set_text("toast", error);
            }
            if update.match_started {
                let settings = session.view().settings;
                self.pending_travel = Some(settings);
                self.menus.set_num("load_progress", 0.0);
                let effect = self.flow.transition(FlowAction::MatchStarted);
                self.menus.apply(effect)?;
            }
            if update.match_ended && !self.match_ended {
                self.match_ended = true;
                let summary = MatchSummary::from_world(world)?;
                self.result_countup_ticks = 0;
                self.menus.set_results(&summary, 0);
                self.match_summary = Some(summary);
                let effect = self.flow.transition(FlowAction::MatchEnded);
                self.menus.apply(effect)?;
            }
            if self.flow == Flow::Lobby && (update.countdown_started || update.countdown_cancelled)
            {
                refresh_lobby = true;
            }
        }
        if refresh_lobby {
            self.bind_menu_data()?;
        }
        if self.flow == Flow::Match
            && self.menus.active_screen(MenuLayer::GameMenu) == Some(Screen::Scoreboard)
        {
            self.refresh_scoreboard(world)?;
        }
        if self.flow == Flow::Results
            && self.result_countup_ticks < RESULT_COUNTUP_TICKS
            && let Some(summary) = &self.match_summary
        {
            self.result_countup_ticks += 1;
            let score = u64::from(summary.player_kills())
                .saturating_mul(u64::from(self.result_countup_ticks))
                / u64::from(RESULT_COUNTUP_TICKS);
            let score = u32::try_from(score).unwrap_or(u32::MAX);
            self.menus.set_result_score(score);
        }
        self.refresh_scene_roles(world)?;
        let tuning_path = AssetPath::new("data/tuning.ron")?;
        let tuning_hash = assets.content_hash(&tuning_path)?;
        if tuning_hash != self.tuning_hash {
            match load_tuning(assets.content_pack()) {
                Ok(tuning) => match apply_tuning(world, tuning) {
                    Ok(()) => {
                        self.tuning_hash = tuning_hash;
                        self.tuning_error.clear();
                    }
                    Err(error) => self.tuning_error.show(error.to_string(), Instant::now()),
                },
                Err(error) => self.tuning_error.show(error.to_string(), Instant::now()),
            }
        }
        let scene_file = if self.active_arena_id == 0 {
            "data/arenas/kiln_yard.level.ron"
        } else {
            "content/cooked/sample_arena.level.ron"
        };
        let scene_path = AssetPath::new(scene_file)?;
        let scene_hash = assets.content_hash(&scene_path)?;
        if scene_hash != self.scene_hash {
            let content = assets.content_pack().clone();
            let previous = world.resource::<crate::scene::SceneContent>()?.0.clone();
            let result = crate::scene::level_from_content(&content, self.active_arena_id).and_then(
                |level| {
                    world.insert_resource(crate::scene::SceneContent(content.clone()))?;
                    crate::scene::install_scene(world, &level)
                },
            );
            match result {
                Ok(()) => {
                    self.scene_hash = scene_hash;
                    self.tuning_error.clear();
                }
                Err(error) => {
                    world.insert_resource(crate::scene::SceneContent(previous))?;
                    self.tuning_error.show(error.to_string(), Instant::now());
                }
            }
        }
        let aim_assist_tuning = world.resource::<Tuning>()?.aim_assist;
        if self.aim_assist_tuning != Some(aim_assist_tuning) {
            self.aim_assist_params = Some(world.resource::<Tuning>()?.aim_assist_params()?);
            self.aim_assist_tuning = Some(aim_assist_tuning);
        }
        self.load_identities(world)?;
        self.snapshot(world)?;
        Ok(())
    }

    fn on_travel(&mut self, _world: &World, mut gfx: Option<&mut Gfx>) -> Result<(), AppError> {
        self.stale_draws.clear();
        self.stale_draws.extend(self.draws.keys().copied());
        for key in self.stale_draws.drain(..) {
            if let Some(state) = self.draws.get(&key).copied() {
                let renderer = gfx.as_deref_mut().ok_or(AppError::ClientNotReady)?;
                renderer.scene_mut().remove(state.primitive)?;
                self.draws.remove(&key);
            }
        }
        self.roles.clear();
        self.actor_ids.clear();
        self.actor_teams.clear();
        self.alive.clear();
        self.tracers.clear();
        self.hud = None;
        self.hud_bindings = HudBindings::default();
        self.controls = Controls::default();
        self.muzzle_flash_ticks = 0;
        self.camera_eye_above_centre = EYE_ABOVE_CENTRE_M;
        self.sliding = false;
        self.scoped = false;
        self.crouch_latched = false;
        self.audio.on_travel();
        if self.flow == Flow::Loading {
            self.menus.set_num("load_progress", 1.0);
            let effect = self.flow.transition(FlowAction::LoadingComplete);
            self.menus.apply(effect)?;
            self.match_ended = false;
            self.match_summary = None;
            self.score_cache.clear();
            self.result_countup_ticks = 0;
        }
        Ok(())
    }

    fn command(&mut self, input: &Input, look: ViewAngles) -> Result<Self::Command, AppError> {
        self.view_angles = look;
        self.latest_look = look;
        self.update_telemetry_toggle(input);
        self.update_menu(input)?;
        let in_match = self.flow == Flow::Match
            && self.menus.active_screen(MenuLayer::GameMenu).is_none()
            && self.menus.active_screen(MenuLayer::Modal).is_none();
        if in_match {
            self.update_scope(input);
        }
        let command = if !in_match {
            MoveCommand::IDLE
        } else {
            let base = Self::command(look, input, self.settings.mode())?;
            self.movement_command(base, input)
        };
        self.snapshot_controls(input);
        Ok(command)
    }

    fn take_travel(&mut self) -> Option<Self::Settings> {
        let settings = self.pending_travel.take()?;
        self.active_arena_id = settings.arena_id;
        Some(settings)
    }

    fn travel_failed(&mut self, _error: &AppError) -> Result<(), AppError> {
        self.pending_travel = None;
        self.local_session = None;
        self.flow = Flow::Home;
        self.menus.set_text("toast", "MATCH COULD NOT LOAD");
        self.menus
            .apply(FlowEffect::Replace(Screen::Home, MenuLayer::Menu))?;
        Ok(())
    }

    fn observe(
        &mut self,
        report: &FrameReport,
        render_scale: f32,
        gpu: Option<&GpuTimes>,
        presentation: platform::PresentationBinding,
    ) {
        self.telemetry = *report;
        self.render_scale = render_scale;
        self.gpu = gpu.copied();
        self.presentation = presentation;
    }

    fn camera(
        &self,
        frame: &RenderFrame,
        alpha: f32,
        look: ViewAngles,
    ) -> Result<Camera, AppError> {
        let id = *self
            .actor_ids
            .get(PLAYER_SLOT)
            .ok_or(AppError::ClientNotReady)?;
        let pose = frame
            .blended(id, alpha)?
            .ok_or(AppError::MissingFrameEntity(id))?;
        Ok(Camera {
            pos: pose.translation() + Vec3::Y * self.camera_eye_above_centre,
            yaw: look.yaw(),
            pitch: look.pitch() + self.presentation_recoil(),
            roll: if self.sliding {
                3.0_f32.to_radians()
            } else {
                0.0
            },
            fov_y: if self.scoped {
                f32::from(self.settings.graphics().fov).to_radians() * SCOPE_FOV_SCALE
            } else {
                f32::from(self.settings.graphics().fov).to_radians()
            },
            ..Camera::default()
        })
    }

    fn play_audio(&mut self, world: &World, audio: &mut Audio) -> Result<(), AppError> {
        for (bus, volume) in [
            (Bus::Master, self.settings.volume()),
            (Bus::Music, self.settings.music_volume()),
            (Bus::Sfx, self.settings.sfx_volume()),
            (Bus::Voice, self.settings.voice_volume()),
        ] {
            audio.set_volume(bus, Volume::new(f32::from(volume) / 100.0)?);
        }
        self.audio.play_frame(world, audio)
    }

    fn draw_ui(
        &mut self,
        gfx: &mut Gfx,
        regions: &mut Vec<TouchRegion>,
        assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        let Some(font) = self.font else {
            return Ok(());
        };
        gfx.set_render_scale(f32::from(self.settings.graphics().render_scale) / 100.0);
        let screen = (gfx.ui.size.x, gfx.ui.size.y);
        let mut presentation_error = None;
        if let Err(error) = self.menus.reload_from_assets(assets) {
            self.tuning_error.show(error.to_string(), Instant::now());
        }
        let hud_path = AssetPath::new("ui/hud.ui.ron")?;
        let hud_hash = assets.content_hash(&hud_path)?;
        if hud_hash != self.hud_hash {
            match assets.content_text(&hud_path) {
                Ok(source) => match self.hud_ui.reload_source(source) {
                    Ok(()) => {
                        self.hud_hash = hud_hash;
                        self.tuning_error.clear();
                    }
                    Err(error) => self.tuning_error.show(error.to_string(), Instant::now()),
                },
                Err(error) => self.tuning_error.show(error.to_string(), Instant::now()),
            }
        }
        if self.flow == Flow::Loading
            && let Err(error) = self.menus.layout_for_draw(gfx.ui.size)
        {
            presentation_error.get_or_insert(error);
        }
        if self.flow == Flow::Match
            && self.menus.active_screen(MenuLayer::GameMenu).is_none()
            && self.menus.active_screen(MenuLayer::Modal).is_none()
            && let Some(hud) = self.hud.as_ref()
        {
            hud::draw_feedback(&mut gfx.ui, hud);
            let bindings = self.hud_bindings.update(hud, self.controls);
            match self.hud_ui.draw(&mut gfx.ui, font, bindings, screen) {
                Ok(drawn) => regions.extend_from_slice(drawn),
                Err(error) => {
                    presentation_error.get_or_insert(error);
                }
            }
        }
        self.menus.draw(&mut gfx.ui, font);
        let view_mode = gfx.view_mode();
        if self.show_telemetry
            && let Some(region) = draw_overlay(
                &mut gfx.ui,
                font,
                &self.telemetry,
                self.render_scale,
                self.gpu.as_ref(),
                view_mode,
                self.presentation,
            )
        {
            regions.push(region);
        }
        self.tuning_error.clear_expired(Instant::now());
        self.tuning_error.draw(&mut gfx.ui, screen.0, font);
        match presentation_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn draw(
        &mut self,
        frame: &RenderFrame,
        alpha: f32,
        gfx: &mut Gfx,
        assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        self.cooked_scene.load_frame_assets(frame, assets)?;
        self.cooked_scene.draw_cooked(frame, alpha, gfx, assets)?;
        self.prepare_aim_scale(frame, alpha)?;
        let cube = self.cube.ok_or(AppError::ClientNotReady)?;
        let capsule = self.capsule.ok_or(AppError::ClientNotReady)?;
        let head = self.head.ok_or(AppError::ClientNotReady)?;
        self.draw_frame = self.draw_frame.wrapping_add(1);
        if self.draw_frame == 0 {
            self.draw_frame = 1;
            for state in self.draws.values_mut() {
                state.frame_seen = 0;
            }
        }
        for entry in frame.transforms() {
            let transform = blend(&entry.previous, &entry.current, alpha)?;
            if let Some(role) = self.role_of(entry.id) {
                self.upsert_primitive(
                    gfx,
                    DrawKey::Entity(entry.id, 0),
                    cube,
                    primitive_desc(
                        transform.matrix(),
                        draw_look(box_tint(role), 0.0),
                        Layer::World,
                        true,
                    ),
                )?;
                continue;
            }
            let Some(slot) = self.slot_of(entry.id) else {
                continue;
            };
            let Some(&alive) = self.alive.get(slot) else {
                continue;
            };
            let Some(&team) = self.actor_teams.get(slot) else {
                continue;
            };
            if slot == PLAYER_SLOT || !alive {
                continue;
            }
            let tint = team_tint(team);
            let model = transform.matrix();
            self.upsert_primitive(
                gfx,
                DrawKey::Entity(entry.id, 0),
                capsule,
                primitive_desc(model, draw_look(tint, 0.0), Layer::World, true),
            )?;
            let head_offset = Mat4::from_translation(Vec3::Y * HEAD_OFFSET_M);
            self.upsert_primitive(
                gfx,
                DrawKey::Entity(entry.id, 1),
                head,
                primitive_desc(
                    model * head_offset,
                    draw_look(tint, 0.0),
                    Layer::World,
                    true,
                ),
            )?;
            let accent = team_accent(team);
            let chest = Mat4::from_scale_rotation_translation(
                Vec3::new(0.34, 0.08, 0.31),
                Quat::IDENTITY,
                Vec3::new(0.0, 0.25, 0.0),
            );
            self.upsert_primitive(
                gfx,
                DrawKey::Entity(entry.id, 2),
                cube,
                primitive_desc(model * chest, draw_look(accent, 1.6), Layer::World, false),
            )?;
            let head_band = Mat4::from_scale_rotation_translation(
                Vec3::new(0.26, 0.07, 0.08),
                Quat::IDENTITY,
                Vec3::new(0.0, 0.88, -0.25),
            );
            self.upsert_primitive(
                gfx,
                DrawKey::Entity(entry.id, 3),
                cube,
                primitive_desc(
                    model * head_band,
                    draw_look(accent, 1.6),
                    Layer::World,
                    false,
                ),
            )?;
        }
        let muzzle = self.draw_viewmodel(frame, alpha, gfx, assets)?;
        self.draw_tracers(gfx, cube, muzzle)?;
        self.retire_unseen(gfx)?;
        Ok(())
    }
}

impl ArenaPresenter {
    fn upsert_primitive(
        &mut self,
        gfx: &mut Gfx,
        key: DrawKey,
        mesh: MeshId,
        desc: PrimitiveDesc,
    ) -> Result<(), AppError> {
        let matrix = Mat4::from(desc.transform);
        let bits = matrix.to_cols_array().map(f32::to_bits);
        let uploader_replacement = desc.layer == Layer::ViewModel
            && matches!(desc.material, render::Material::Textured(_));
        if self
            .draws
            .get(&key)
            .is_some_and(|state| state.mesh != mesh && !uploader_replacement)
            && let Some(old) = self.draws.remove(&key)
        {
            gfx.scene_mut().remove(old.primitive)?;
        }
        if let Some(state) = self.draws.get_mut(&key) {
            state.mesh = mesh;
            if state.transform_bits != bits {
                gfx.scene_mut()
                    .set_transform(state.primitive, Affine3A::from_mat4(matrix))?;
                state.transform_bits = bits;
            }
            if let render::Material::Procedural(look) = desc.material
                && state.look != look
            {
                gfx.scene_mut().set_look(state.primitive, look)?;
                state.look = look;
            }
            state.frame_seen = self.draw_frame;
            return Ok(());
        }
        let primitive = gfx.scene_mut().add(mesh, desc)?;
        self.draws.insert(
            key,
            DrawState {
                primitive,
                mesh,
                look: match desc.material {
                    render::Material::Procedural(look) => look,
                    render::Material::Textured(_) => RenderLook::default(),
                },
                transform_bits: bits,
                frame_seen: self.draw_frame,
            },
        );
        Ok(())
    }

    fn retire_unseen(&mut self, gfx: &mut Gfx) -> Result<(), AppError> {
        self.stale_draws.clear();
        self.stale_draws.extend(
            self.draws
                .iter()
                .filter_map(|(key, state)| (state.frame_seen != self.draw_frame).then_some(*key)),
        );
        for key in self.stale_draws.drain(..) {
            if let Some(state) = self.draws.remove(&key) {
                gfx.scene_mut().remove(state.primitive)?;
            }
        }
        Ok(())
    }

    /// Draws each young tracer as a thin emissive box from the muzzle to the hit, and a light at
    /// the muzzle while the flash lasts.
    fn draw_tracers(
        &mut self,
        gfx: &mut Gfx,
        cube: MeshId,
        muzzle: Option<Vec3>,
    ) -> Result<(), AppError> {
        let Some(muzzle) = muzzle else {
            return Ok(());
        };
        for slot in 0..self.tracers.len() {
            let Some(tracer) = self.tracers.get(slot).copied().flatten() else {
                continue;
            };
            let from = muzzle;
            let delta = tracer.to() - from;
            let length = delta.length();
            if length > 1e-3 {
                let rotation = Quat::from_rotation_arc(Vec3::Z, delta / length);
                let model = Mat4::from_scale_rotation_translation(
                    Vec3::new(TRACER_WIDTH_M, TRACER_WIDTH_M, length),
                    rotation,
                    from + delta * 0.5,
                );
                let Some(&id) = self.actor_ids.get(slot) else {
                    continue;
                };
                self.upsert_primitive(
                    gfx,
                    DrawKey::Tracer(id),
                    cube,
                    primitive_desc(
                        model,
                        draw_look(TRACER_COLOUR, TRACER_EMISSIVE),
                        Layer::Glow,
                        false,
                    ),
                )?;
            }
            if tracer.age() < self.muzzle_flash_ticks {
                gfx.light(
                    from,
                    MUZZLE_LIGHT_RADIUS_M,
                    MUZZLE_LIGHT_COLOUR,
                    MUZZLE_LIGHT_INTENSITY,
                );
            }
        }
        Ok(())
    }

    /// Draws the cooked pistol and returns its world-space muzzle socket when it is ready.
    fn draw_viewmodel(
        &mut self,
        frame: &RenderFrame,
        alpha: f32,
        gfx: &mut Gfx,
        assets: &mut ClientAssets<'_>,
    ) -> Result<Option<Vec3>, AppError> {
        let Some(&alive) = self.alive.get(PLAYER_SLOT) else {
            return Ok(None);
        };
        if !alive {
            return Ok(None);
        }
        let Some(handle) = self.pistol.as_ref() else {
            return Ok(None);
        };
        let UploadStatus::Ready(gpu_pack) = assets.uploader.status(handle) else {
            return Ok(None);
        };
        let Some(pack) = assets.current(handle)? else {
            return Ok(None);
        };
        let camera = self.camera(frame, alpha, self.view_angles)?;
        let camera_rotation = Quat::from_rotation_y(camera.yaw)
            * Quat::from_rotation_x(camera.pitch)
            * Quat::from_rotation_z(camera.roll);
        // The viewmodel projection already uses camera space, so only the muzzle needs world space.
        let viewmodel_local = Mat4::from_scale_rotation_translation(
            Vec3::ONE,
            Quat::from_rotation_y(std::f32::consts::PI),
            Vec3::new(0.22, 0.02, -0.48),
        );
        let viewmodel_world =
            Mat4::from_translation(camera.pos) * Mat4::from_quat(camera_rotation) * viewmodel_local;
        for (mesh_index, mesh) in pack.meshes.iter().enumerate() {
            let Some(gpu_mesh) = gpu_pack.meshes.get(mesh_index).copied() else {
                continue;
            };
            let Some(material_index) = usize::try_from(mesh.material).ok() else {
                continue;
            };
            let Some(material) = gpu_pack.materials.get(material_index).copied() else {
                continue;
            };
            let Some(node_index) = usize::try_from(mesh.node).ok() else {
                continue;
            };
            let Some(node_transform) = pack.node_world_transform(node_index) else {
                continue;
            };
            let Some(part) = u8::try_from(mesh_index).ok() else {
                continue;
            };
            let transform = viewmodel_local * node_transform;
            self.upsert_primitive(
                gfx,
                DrawKey::ViewModel(part),
                gpu_mesh,
                PrimitiveDesc {
                    transform: Affine3A::from_mat4(transform),
                    material: render::Material::Textured(material),
                    layer: Layer::ViewModel,
                    casts_shadow: false,
                },
            )?;
        }
        let muzzle_node = pack.sockets().find_map(|(name, index)| {
            (name.eq_ignore_ascii_case("muzzle") || name.eq_ignore_ascii_case("socket_muzzle"))
                .then_some(index)
        });
        let muzzle = muzzle_node
            .and_then(|index| pack.node_world_transform(index))
            .map(|socket| (viewmodel_world * socket).transform_point3(Vec3::ZERO));
        Ok(muzzle)
    }
}

fn draw_look(tint: [f32; 3], emissive: f32) -> RenderLook {
    RenderLook {
        tint: [tint[0], tint[1], tint[2], 1.0],
        emissive,
        roughness: None,
        metalness: None,
        pattern: None,
        object_space_pattern: false,
        unlit: false,
    }
}

fn primitive_desc(
    matrix: Mat4,
    look: RenderLook,
    layer: Layer,
    casts_shadow: bool,
) -> PrimitiveDesc {
    PrimitiveDesc {
        transform: Affine3A::from_mat4(matrix),
        material: render::Material::Procedural(look),
        layer,
        casts_shadow,
    }
}

fn session_error_label(error: &crate::session::LocalSessionError) -> &'static str {
    match error {
        crate::session::LocalSessionError::BotCapacityReached => "ROOM IS FULL",
        crate::session::LocalSessionError::BotRosterExhausted => "BOT NAME UNAVAILABLE",
        crate::session::LocalSessionError::ModeUnavailable
        | crate::session::LocalSessionError::UnsupportedMode => "MODE UNAVAILABLE",
        crate::session::LocalSessionError::Session(_) => "ROOM ACTION REFUSED",
        crate::session::LocalSessionError::LocalPlayerNotSeated => "ROOM COULD NOT OPEN",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use engine_ui::{Document, MetricsKey, TextMeasure, UiLayout};
    use platform::{ConfigName, ConfigStore};

    struct ApproxMeasure;

    impl TextMeasure for ApproxMeasure {
        fn metrics_key(&self) -> MetricsKey {
            MetricsKey(1)
        }

        fn width(&mut self, px: f32, text: &str) -> f32 {
            px * 0.55 * text.chars().count() as f32
        }
    }

    fn cooked_hud_input() -> Result<Input, Box<dyn std::error::Error>> {
        let content = crate::test_support::pack().expect("test pack");
        let path = AssetPath::new("ui/hud.ui.ron")?;
        let document = Document::from_ron(content.load_text(&path)?)?;
        let mut layout = UiLayout::new(document)?;
        let mut bindings = HudBindings::default();
        let values = bindings.update(
            &HudState::default(),
            Controls {
                touch: true,
                left_fire_setting: true,
                ..Controls::default()
            },
        );
        let mut measure = ApproxMeasure;
        let drawn = layout.frame(values, (1600.0, 900.0), &mut measure)?;
        let mut input = Input::default();
        input.screen = Vec2::new(1600.0, 900.0);
        assert!(input.set_touch_regions(drawn.touch_regions()));
        Ok(input)
    }

    fn region_centre(input: &Input, id: platform::TouchRegionId) -> Result<Vec2, &'static str> {
        let region = input
            .touch_regions()
            .iter()
            .find(|region| region.id == id)
            .ok_or("HUD did not draw the requested touch region")?;
        let rect = region.rect.rect();
        Ok(Vec2::new(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5))
    }

    #[test]
    fn cooked_hud_regions_drive_fire_jump_reload_and_scope()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut input = cooked_hud_input()?;
        let fire = region_centre(&input, touch::FIRE)?;
        input.touch_start(1, fire);
        input.touch_move(1, fire + Vec2::new(24.0, -20.0));
        let fire_command = ArenaPresenter::command(ViewAngles::ZERO, &input, FireMode::Manual)?;
        assert!(fire_command.fire());
        assert_eq!(input.touch_look, Vec2::new(24.0, -20.0));
        input.touch_end(1);

        input.end_frame();
        let jump = region_centre(&input, touch::JUMP)?;
        input.touch_start(2, jump);
        assert!(
            ArenaPresenter::command(ViewAngles::ZERO, &input, FireMode::Manual)?.jump_pressed()
        );
        input.end_frame();
        assert!(
            !ArenaPresenter::command(ViewAngles::ZERO, &input, FireMode::Manual)?.jump_pressed()
        );
        input.touch_end(2);

        input.end_frame();
        let reload = region_centre(&input, touch::RELOAD)?;
        input.touch_start(3, reload);
        assert!(ArenaPresenter::command(ViewAngles::ZERO, &input, FireMode::Manual)?.reload());
        input.touch_end(3);

        input.end_frame();
        let scope = region_centre(&input, touch::SCOPE)?;
        let content = crate::test_support::pack().expect("test pack");
        let mut presenter = ArenaPresenter::new(&content, true)?;
        presenter.flow = Flow::Match;
        input.touch_start(4, scope);
        presenter.update_scope(&input);
        assert!(presenter.scoped);
        assert!(presenter.look_scale() < 1.0);
        Ok(())
    }

    #[test]
    fn cooked_hud_left_fire_does_not_turn_the_view() -> Result<(), Box<dyn std::error::Error>> {
        let mut input = cooked_hud_input()?;
        let left_fire = region_centre(&input, touch::FIRE_LEFT)?;
        input.touch_start(1, left_fire);
        input.touch_move(1, left_fire + Vec2::new(30.0, -20.0));
        assert!(ArenaPresenter::command(ViewAngles::ZERO, &input, FireMode::Manual)?.fire());
        assert_eq!(input.touch_look, Vec2::ZERO);
        Ok(())
    }

    #[test]
    fn settings_persist_through_the_engine_store() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("imagine-settings-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        let store = ConfigStore::from_root(&root);
        let name = ConfigName::new("settings.ron")?;
        let mut initial = Settings::default();
        initial.nudge_x(2);
        initial.nudge_y(-1);
        initial.set_range(SettingControl::Volume, 70.0);
        initial.toggle_mode();
        let mut seed = SettingsFile::new(store.clone(), name.clone());
        seed.save_if_changed(&initial)?;
        let content = crate::test_support::pack().expect("test pack");
        let mut presenter = ArenaPresenter::with_settings_file(
            &content,
            true,
            SettingsFile::new(store.clone(), name.clone()),
        )?;
        assert_eq!(
            (presenter.settings.sens_x(), presenter.settings.sens_y()),
            (12, 9)
        );
        assert_eq!(presenter.settings.mode(), FireMode::Auto);
        presenter.settings.nudge_x(1);
        presenter.save_settings();

        let mut saved = SettingsFile::<Settings>::new(store, name);
        let (settings, warning) = saved.load();
        assert_eq!(warning, None);
        assert_eq!((settings.sens_x(), settings.sens_y()), (13, 9));
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn observe_retains_current_frame_telemetry() -> Result<(), AppError> {
        let content = crate::test_support::pack().expect("test pack");
        let mut presenter = ArenaPresenter::new(&content, false)?;
        let report = FrameReport {
            fps: 118.0,
            ..FrameReport::default()
        };
        let gpu = GpuTimes {
            frame: 3.5,
            ..GpuTimes::default()
        };
        let binding = platform::PresentationBinding {
            intended_present_hz: Some(120.0),
            declared: true,
        };
        presenter.observe(&report, 0.75, Some(&gpu), binding);
        assert_eq!(presenter.telemetry, report);
        assert_eq!(presenter.render_scale, 0.75);
        assert_eq!(presenter.gpu, Some(gpu));
        presenter.observe(&report, 1.0, None, binding);
        assert_eq!(presenter.gpu, None);
        Ok(())
    }
}
