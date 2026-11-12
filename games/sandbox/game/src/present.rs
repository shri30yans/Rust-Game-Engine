//! The sandbox presenter: keyboard and mouse in, the scene drawn by the engine's scene presenter,
//! and a first-person camera. A changed mounted pack reloads its level entry through this presenter.
//!
//! The presenter turns keyboard state into the movement axes of a [`PlayerCommand`], passes the
//! engine's view angles through as the command's heading, and asks the engine for the mouse grab,
//! so mouse motion turns the view. On touch the JUMP button is the jump and is drawn as a plain
//! ring and disc. The frame telemetry panel sits at the top left: F3 toggles it on desktop,
//! and on touch it starts shown and an invisible button over its corner toggles it. The character
//! itself is never drawn; the camera sits at its eye and uses the same view angles the simulation
//! faces the character with.
//!
//! Simulation setup reads the initial level from its mounted pack. This presenter checks the pack
//! hash before ticks, reinstalls a valid changed level, and keeps the last good scene on failure.
//!
//! Invariants: the camera fails rather than guessing when the character is absent from the frame;
//! a rejected pack change never changes the running scene.

use app::{
    AppError, ClientAssets, ErrorBanner, FrameReport, LookDelta, Presenter, RenderFrame,
    ScenePresenter, TELEMETRY_TOUCH_ID, ViewAngles, draw_overlay,
};
use engine_assets::AssetPath;
use engine_world::{NetId, World};
use glam::{Vec2, Vec3};
use platform::{Code, Input, Key, Rect, TouchRegion, TouchRegionId, TouchShape, Viewport};
use render::{Camera, FontId, Gfx, GpuTimes, rgba};

use crate::sim::{Axis, CENTRE_ABOVE_FEET_M, EYE_HEIGHT_M, Player, PlayerCommand};
use std::time::Instant;

/// The touch region identifier for jump.
pub const JUMP: TouchRegionId = TouchRegionId(0);

/// The visible JUMP region for a screen. It is constructed only after clipping to that screen.
fn jump_region(screen: Vec2) -> Option<TouchRegion> {
    let diameter = screen.y * 0.24;
    Viewport::new(screen.x, screen.y)?
        .clip(Rect {
            x: screen.x * 0.88 - diameter * 0.5,
            y: screen.y * 0.72 - diameter * 0.5,
            w: diameter,
            h: diameter,
        })
        .map(|rect| TouchRegion {
            id: JUMP,
            rect,
            shape: TouchShape::Circle,
            drag_look: false,
        })
}

/// Whether the character should jump this frame: the keyboard's fire key or Space, or a finger on
/// the JUMP region.
#[must_use]
pub fn jump_held(input: &Input) -> bool {
    input.fire() || input.down(Code::Space) || input.touch_region_held(JUMP)
}

/// Draws the sandbox scene and turns keyboard and mouse state into player commands.
pub struct SandboxPresenter {
    scene: ScenePresenter,
    banner: ErrorBanner,
    /// The character's id; the camera follows it. Set by the first `apply_input`.
    player: Option<NetId>,
    /// Whether the device has touch input, so the JUMP region is drawn.
    show_touch_buttons: bool,
    /// The font the telemetry panel is drawn in, loaded in `setup`.
    font: Option<FontId>,
    level_hash: Option<u64>,
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
    /// Optional authored view adjustment used by scene previews.
    camera_offset: LookDelta,
}

impl Default for SandboxPresenter {
    fn default() -> Self {
        Self {
            scene: ScenePresenter::default(),
            banner: ErrorBanner::default(),
            player: None,
            show_touch_buttons: false,
            font: None,
            level_hash: None,
            show_telemetry: false,
            telemetry: FrameReport::default(),
            render_scale: 1.0,
            gpu: None,
            presentation: platform::PresentationBinding::default(),
            camera_offset: LookDelta::ZERO,
        }
    }
}

impl SandboxPresenter {
    /// A presenter whose UI draw emits the JUMP and telemetry regions. The JUMP region is drawn
    /// only when `show_touch_buttons` is set. The telemetry panel starts shown on touch and hidden
    /// on desktop.
    #[must_use]
    pub fn new(show_touch_buttons: bool) -> Self {
        Self {
            show_touch_buttons,
            show_telemetry: show_touch_buttons,
            ..Self::default()
        }
    }

    /// Selects whether the changing frame telemetry panel is drawn.
    #[must_use]
    pub fn with_telemetry(mut self, show_telemetry: bool) -> Self {
        self.show_telemetry = show_telemetry;
        self
    }

    /// Adds an authored view adjustment to the live look before building the camera.
    #[must_use]
    pub fn with_camera_offset(mut self, offset: LookDelta) -> Self {
        self.camera_offset = offset;
        self
    }
}

/// Stick deflection, in [-1, 1], that a press needs. Smaller deflections are read as centred, so a
/// thumb resting near the middle of the stick does not walk.
pub const STICK_DEAD_ZONE: f32 = 0.3;

/// The (positive, negative) presses that a stick deflection makes beyond the dead zone.
#[must_use]
pub fn stick_presses(deflection: f32) -> (bool, bool) {
    (deflection > STICK_DEAD_ZONE, deflection < -STICK_DEAD_ZONE)
}

impl Presenter for SandboxPresenter {
    type Command = PlayerCommand;
    type Settings = ();

    fn setup(&mut self, gfx: &mut Gfx, assets: &mut ClientAssets<'_>) -> Result<(), AppError> {
        self.scene.setup(gfx)?;
        let font_path = AssetPath::new("fonts/Roboto-Medium.ttf")?;
        self.font = Some(gfx.ui.atlas.add_font(assets.content_bytes(&font_path)?)?);
        self.level_hash = Some(assets.content_hash(&AssetPath::new("game/levels/sandbox.ron")?)?);
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

    fn grab_mouse(&self) -> bool {
        true
    }

    fn before_ticks(
        &mut self,
        world: &mut World,
        assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        self.banner.clear_expired(Instant::now());
        let path = AssetPath::new("game/levels/sandbox.ron")?;
        let hash = assets.content_hash(&path)?;
        if self.level_hash != Some(hash) {
            let content = assets.content_pack().clone();
            let result = crate::scene::load_sandbox_level(&content).and_then(|level| {
                world.insert_resource(crate::scene::SceneContent(content.clone()))?;
                crate::scene::install_scene(world, &level)
            });
            match result {
                Ok(()) => {
                    self.level_hash = Some(hash);
                    self.banner.clear();
                }
                Err(error) => self.banner.show(error.to_string(), Instant::now()),
            }
        }
        self.player = Some(world.resource::<Player>()?.id());
        Ok(())
    }

    fn command(&mut self, input: &Input, look: ViewAngles) -> Result<Self::Command, AppError> {
        if input.hit(Code::F3) || input.touch_region_pressed(TELEMETRY_TOUCH_ID) {
            self.show_telemetry = !self.show_telemetry;
        }
        // The touch stick and the keyboard both press the same axes; either one moves the character.
        let stick = input.stick();
        let (stick_right, stick_left) = stick_presses(stick.x);
        let (stick_forward, stick_back) = stick_presses(stick.y);
        let strafe = Axis::from_presses(
            input.down(Code::D) || stick_right,
            input.down(Code::A) || stick_left,
        );
        let forward = Axis::from_presses(
            input.down(Code::W) || input.held(Key::Up) || stick_forward,
            input.down(Code::S) || input.held(Key::Down) || stick_back,
        );
        let jump = jump_held(input);
        Ok(PlayerCommand::new(strafe, forward, jump, look))
    }

    fn camera(
        &self,
        frame: &RenderFrame,
        alpha: f32,
        look: ViewAngles,
    ) -> Result<Camera, AppError> {
        let id = self.player.ok_or(AppError::ClientNotReady)?;
        let pose = frame
            .blended(id, alpha)?
            .ok_or(AppError::MissingFrameEntity(id))?;
        let look = look.turned(self.camera_offset);
        Ok(Camera {
            pos: pose.translation() + Vec3::Y * (EYE_HEIGHT_M - CENTRE_ABOVE_FEET_M),
            yaw: look.yaw(),
            pitch: look.pitch(),
            ..Camera::default()
        })
    }

    fn draw_ui(
        &mut self,
        gfx: &mut Gfx,
        regions: &mut Vec<TouchRegion>,
        _assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        if self.show_touch_buttons
            && let Some(region) = jump_region(gfx.ui.size)
        {
            let rect = region.rect.rect();
            draw_jump_button(
                &mut gfx.ui,
                Vec2::new(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5),
                rect.w.min(rect.h) * 0.5,
                false,
            );
            regions.push(region);
        }
        let view_mode = gfx.view_mode();
        if let (Some(font), true) = (self.font, self.show_telemetry)
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
        if let Some(font) = self.font {
            self.banner.draw(&mut gfx.ui, gfx.size.0 as f32, font);
        }
        Ok(())
    }

    fn draw(
        &mut self,
        frame: &RenderFrame,
        alpha: f32,
        gfx: &mut Gfx,
        _assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        self.scene.load_frame_assets(frame, _assets)?;
        self.scene.draw(frame, alpha, gfx, _assets)?;
        if let Some(font) = self.font {
            self.banner.draw(&mut gfx.ui, gfx.size.0 as f32, font);
        }
        Ok(())
    }
}

/// Draws the JUMP button as a disc with a ring round it. A held button is brighter.
fn draw_jump_button(ui: &mut render::Ui, centre: Vec2, radius: f32, held: bool) {
    let (fill, ring) = if held { (0.35, 0.95) } else { (0.15, 0.6) };
    ui.circle(centre.x, centre.y, radius, rgba(0xffffff, fill));
    ui.ring(
        centre.x,
        centre.y,
        radius,
        (radius * 0.08).max(2.0),
        1.0,
        rgba(0xffffff, ring),
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn touch_input() -> Input {
        let screen = Vec2::new(1000.0, 1000.0);
        let mut input = Input::default();
        input.screen = screen;
        let region = jump_region(screen).expect("square viewport has a visible jump region");
        assert!(input.set_touch_regions(&[region]));
        input
    }

    #[test]
    fn stick_dead_zone_requires_a_clear_deflection() {
        for deflection in [0.0, 0.1, -0.2, STICK_DEAD_ZONE, -STICK_DEAD_ZONE] {
            assert_eq!(stick_presses(deflection), (false, false), "{deflection}");
        }
        assert_eq!(stick_presses(0.31), (true, false));
        assert_eq!(stick_presses(-0.31), (false, true));
    }

    #[test]
    fn drawn_jump_region_jumps_while_held() {
        let mut input = touch_input();
        let region = input
            .touch_regions()
            .first()
            .copied()
            .expect("jump region was installed");
        let rect = region.rect.rect();
        let centre = Vec2::new(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5);
        input.touch_start(1, centre);
        assert!(jump_held(&input));
        input.touch_end(1);
        assert!(!jump_held(&input));
    }

    #[test]
    fn touch_telemetry_starts_visible_and_toggles_from_its_region()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut touch = SandboxPresenter::new(true);
        let desktop = SandboxPresenter::new(false);
        assert!(touch.show_telemetry);
        assert!(!desktop.show_telemetry);

        let mut input = Input::default();
        input.screen = Vec2::new(1000.0, 1000.0);
        let region = Viewport::new(1000.0, 1000.0)
            .and_then(|viewport| {
                viewport.clip(Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                })
            })
            .map(|rect| TouchRegion {
                id: TELEMETRY_TOUCH_ID,
                rect,
                shape: TouchShape::Rect,
                drag_look: false,
            })
            .ok_or("test telemetry region must be visible")?;
        assert!(input.set_touch_regions(&[region]));
        input.touch_start(1, Vec2::new(50.0, 50.0));
        let _ = Presenter::command(&mut touch, &input, ViewAngles::ZERO)?;
        assert!(!touch.show_telemetry);
        input.end_frame();
        let _ = Presenter::command(&mut touch, &input, ViewAngles::ZERO)?;
        assert!(!touch.show_telemetry, "a held region is not another press");
        Ok(())
    }
}
