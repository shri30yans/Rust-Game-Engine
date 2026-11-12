//! The single-threaded client runner: one window, one thread.
//! Each rendered frame it applies the late camera, runs the fixed ticks the elapsed time owes,
//! extracts the result when a tick ran, and draws the blend of the last two ticks.
//! With a governor (`with_governor`), each frame's wall time may change the render scale after the
//! frame is drawn.
//!
//! The game supplies its drawing and its input handling through [`Presenter`]; the engine never
//! names a game. [`PlatformApp`] callbacks cannot return errors, so recoverable frame errors
//! are reported in an engine-owned banner and skipped. Only unrecoverable GPU setup/device
//! failures and standalone simulation failures stop the client. UI layout, binding and drawing
//! errors keep the last good interface and input regions and never consume the 120-frame budget.
//!
//! Invariants: a frame is drawn only after the GPU is set up; the front frame always holds the
//! newest extract and is never partly written (extraction fills the back frame and swaps it in
//! only on success); no allocation happens in `simulate` or the telemetry step after construction;
//! no threads. The one clock read is the runner's own timing of `simulate` for the telemetry
//!: it is measured here, never fed into the simulation.

use std::time::{Duration, Instant};

use engine_assets::{AssetStore, ContentPack};
use engine_audio::Audio;
use engine_scene::SceneContent;
use engine_world::World;
use jobs::JobPool;
use platform::{DeviceProfile, Gpu, Input, PlatformApp, PresentationBinding, TouchRegion};
use render::{Camera, FontId, Gfx, Governor, GovernorConfig, GpuTimes, Settings};

use crate::app::App;
use crate::asset_uploader::{AssetUploader, ClientAssets};
use crate::clock::TickRate;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::error_banner::ErrorBanner;
use crate::extract::{Extractor, RenderFrame, entity_capacity};
use crate::game::NetGame;
use crate::game::{Game, PlayerId};
use crate::late_camera::{LookSensitivity, ViewAngles};
use crate::live_scene::LiveScene;
use crate::net_client::NetClient;
use crate::telemetry::{FrameReport, Telemetry};
use engine_netcode::Transport;

/// An online client session adapted to the presenter's command path.
pub trait ClientSession<G: Game> {
    /// Processes one local client tick and any received server data.
    fn tick(&mut self, world: &mut World, command: G::Command) -> Result<(), AppError>;
    /// Takes match settings when the room transitions into a match.
    fn take_pending_travel(&mut self) -> Option<G::Settings>;
    /// Validates and stages game bounds before the replacement world is built.
    fn prepare_travel(&mut self, game: &G, settings: &G::Settings) -> Result<(), AppError>;
    /// Updates the snapshot epoch after the client world has traveled.
    fn set_travel_epoch(&mut self, epoch: u32);
}

impl<G: NetGame, T: Transport> ClientSession<G> for NetClient<G, T> {
    fn tick(&mut self, world: &mut World, command: G::Command) -> Result<(), AppError> {
        NetClient::tick(self, world, command)
    }

    fn take_pending_travel(&mut self) -> Option<G::Settings> {
        NetClient::take_pending_travel(self)
    }

    fn prepare_travel(&mut self, game: &G, settings: &G::Settings) -> Result<(), AppError> {
        NetClient::prepare_travel(self, game, settings)
    }

    fn set_travel_epoch(&mut self, epoch: u32) {
        NetClient::set_travel_epoch(self, epoch);
    }
}

/// Whether the client is a standalone local session or follows a server session.
pub enum Session<G: Game> {
    /// One local player, using the game's normal command path.
    Standalone,
    /// A network client session using the same presenter command type.
    Online(Box<dyn ClientSession<G>>),
}

const ERROR_LOG_INTERVAL: Duration = Duration::from_secs(1);

enum FrameFailure {
    Stop(AppError),
    Skip(AppError),
}

/// What a game supplies to be drawn and controlled. The engine never names a game.
pub trait Presenter {
    /// One tick of game intent produced from the current input state.
    type Command: engine_netcode::InputPayload;
    /// Settings type used by the game when it builds a world.
    type Settings: Clone;
    /// Called once with the renderer, to upload meshes and fonts.
    ///
    /// # Errors
    ///
    /// Any error stops the client; it is returned from [`run_client`].
    fn setup(&mut self, gfx: &mut Gfx, assets: &mut ClientAssets<'_>) -> Result<(), AppError>;

    /// Converts this frame's input into a game command without reading or changing the world.
    ///
    /// # Errors
    ///
    /// A failure skips this frame.
    fn command(&mut self, input: &Input, look: ViewAngles) -> Result<Self::Command, AppError>;

    /// Called once per rendered frame after input preparation and before simulation ticks.
    ///
    /// Use this for presentation-time development work. Fixed-tick work belongs in
    /// [`Presenter::before_ticks`].
    fn before_frame(&mut self, _world: &mut World) -> Result<(), AppError> {
        Ok(())
    }

    /// Acquires every cooked asset referenced by the extracted frame before the uploader runs.
    ///
    /// The engine calls this after extraction and before [`AssetUploader::update`]. Presenters
    /// that draw no scene visuals keep the default implementation.
    fn load_frame_assets(
        &mut self,
        _frame: &RenderFrame,
        _content: &SceneContent,
        _assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        Ok(())
    }

    /// Applies per-tick development updates before the game command, such as live tuning reloads.
    fn before_ticks(
        &mut self,
        _world: &mut World,
        _assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        Ok(())
    }

    /// Clears presenter state that belongs to the old world after travel succeeds. The renderer
    /// is absent only during the initial call before renderer setup; after travel it is supplied
    /// so retained primitives owned by the presenter can be removed.
    fn on_travel(&mut self, _world: &World, _gfx: Option<&mut Gfx>) -> Result<(), AppError> {
        Ok(())
    }

    /// Takes game settings for a client travel requested by the presenter's frame update.
    ///
    /// The client polls once after [`Presenter::command`] and before simulation ticks. Online
    /// travel is server-controlled and rejects a presenter request.
    fn take_travel(&mut self) -> Option<Self::Settings> {
        None
    }

    /// Restores the presenter's front-end state when requested world setup fails.
    fn travel_failed(&mut self, _error: &AppError) -> Result<(), AppError> {
        Ok(())
    }

    /// Whether the window should grab the mouse this frame. While it is grabbed the engine turns
    /// [`ViewAngles`] from mouse motion and passes them to [`Presenter::command`] and
    /// [`Presenter::camera`]; while it is not, the view does not change from the mouse.
    fn grab_mouse(&self) -> bool;

    /// The look sensitivity this frame, when the game has its own (a settings menu). The default
    /// is `None`, which keeps the client's own sensitivity.
    fn look_sensitivity(&self) -> Option<LookSensitivity> {
        None
    }

    /// Multiplies this frame's look input, for example below 1.0 while the game is zoomed in so
    /// a drag turns the picture by the same amount on screen. The default is 1.0.
    fn look_scale(&self) -> f32 {
        1.0
    }

    /// The camera for this frame, from the blended frame, `alpha` in `[0, 1]` and the view.
    ///
    /// # Errors
    ///
    /// Returns an error when the frame cannot give a camera (for example a game's entity is
    /// missing). The current frame is skipped and the error is shown by the engine.
    fn camera(&self, frame: &RenderFrame, alpha: f32, look: ViewAngles)
    -> Result<Camera, AppError>;

    /// Plays sounds after simulation advances. Presenters that have no sound return successfully.
    fn play_audio(&mut self, _world: &World, _audio: &mut Audio) -> Result<(), AppError> {
        Ok(())
    }

    /// Called once per rendered frame, after the camera and before `draw`, with the frame
    /// telemetry and the render scale in effect. `gpu` is the latest GPU timing, or `None`
    /// when the adapter has no timestamp queries; results arrive a frame or two late. The default
    /// ignores them.
    fn observe(
        &mut self,
        _report: &FrameReport,
        _render_scale: f32,
        _gpu: Option<&GpuTimes>,
        _presentation: PresentationBinding,
    ) {
    }

    /// Queues this frame's draws. `frame.blended(id, alpha)` gives each entity's transform
    /// between the last two ticks.
    ///
    /// # Errors
    ///
    /// A failure skips this frame.
    fn draw(
        &mut self,
        frame: &RenderFrame,
        alpha: f32,
        gfx: &mut Gfx,
        assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError>;

    /// Queues this frame's 2D interface after [`Presenter::draw`] and appends the clipped regions
    /// that were actually drawn. The client installs these only after this method succeeds, so the
    /// first frame has no touch regions and a failed UI keeps the last good input layout.
    ///
    /// # Errors
    ///
    /// A UI failure is presentation-only: the client logs and banners it but continues.
    fn draw_ui(
        &mut self,
        _gfx: &mut Gfx,
        _regions: &mut Vec<TouchRegion>,
        _assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        Ok(())
    }
}

/// A game running in the single-threaded client: the app, the presenter, the renderer and the
/// per-frame state. Build it with [`ClientApp::new`], then pass it to [`run_client`].
pub struct ClientApp<G: Game, P: Presenter<Command = G::Command, Settings = G::Settings>> {
    app: App<G>,
    scene_content: SceneContent,
    live_scene: LiveScene,
    presenter: P,
    session: Session<G>,
    pending_command: Option<G::Command>,
    audio: Option<Audio>,
    audio_failure_logged: bool,
    settings: Settings,
    tick_rate: TickRate,
    gfx: Option<Gfx>,
    asset_store: AssetStore,
    asset_uploader: AssetUploader,
    jobs: JobPool,
    extractor: Extractor,
    /// The newest extract; what the presenter draws.
    front: RenderFrame,
    /// Extraction target; swapped into `front` only after a successful extract.
    back: RenderFrame,
    /// Whether `front` holds an extract yet (the first frame extracts even without ticks).
    extracted: bool,
    look: ViewAngles,
    sensitivity: LookSensitivity,
    error: Option<AppError>,
    failed: bool,
    /// Optional dynamic resolution: picks the render scale from the frame times (see
    /// [`ClientApp::with_governor`]).
    governor: Option<Governor>,
    /// Regions from the most recent successful UI draw, reused without per-frame allocation.
    drawn_touch_regions: Vec<TouchRegion>,
    /// Frame times and simulation costs of the last frames.
    telemetry: Telemetry,
    /// Present intent for this surface session, set when the swapchain is configured.
    presentation: PresentationBinding,
    /// The simulation cost of the most recent `simulate`, in milliseconds.
    sim_ms: f32,
    error_banner: ErrorBanner,
    error_font: Option<FontId>,
    last_logged_error: Option<(String, Instant)>,
}

impl<G: Game + 'static, P: Presenter<Command = G::Command, Settings = G::Settings>>
    ClientApp<G, P>
{
    /// Builds the app and the per-frame storage. No GPU is needed yet; `init` builds the renderer.
    ///
    /// # Errors
    ///
    /// Returns errors from [`App::new`] or when the loaded world and game's dynamic budget cannot
    /// be represented as a frame capacity.
    pub fn new(
        game: G,
        game_settings: &G::Settings,
        presenter: P,
        config: &AppConfig,
        profile: &DeviceProfile,
        settings: Settings,
    ) -> Result<Self, AppError> {
        Self::build(
            game,
            game_settings,
            presenter,
            config,
            profile,
            settings,
            Session::Standalone,
        )
    }

    /// Builds an online client without joining a standalone local player.
    pub fn new_online<T: Transport + 'static>(
        game: G,
        game_settings: &G::Settings,
        presenter: P,
        config: &AppConfig,
        profile: &DeviceProfile,
        settings: Settings,
        client: NetClient<G, T>,
    ) -> Result<Self, AppError>
    where
        G: NetGame,
    {
        Self::build(
            game,
            game_settings,
            presenter,
            config,
            profile,
            settings,
            Session::Online(Box::new(client)),
        )
    }

    fn build(
        game: G,
        game_settings: &G::Settings,
        mut presenter: P,
        config: &AppConfig,
        profile: &DeviceProfile,
        settings: Settings,
        mut session: Session<G>,
    ) -> Result<Self, AppError> {
        if let Session::Online(session) = &mut session {
            session.prepare_travel(&game, game_settings)?;
        }
        let scene_content = game.scene_content()?;
        let mut app = App::new(game, game_settings, config, profile)?;
        if matches!(session, Session::Standalone) {
            let local_name = engine_sessions::PlayerName::new("Local")?;
            app.join_player(PlayerId::LOCAL, &local_name)?;
        } else if let Session::Online(session) = &mut session {
            session.set_travel_epoch(app.travel_epoch());
        }
        let capacity = entity_capacity(app.world(), app.game().dynamic_entity_budget())?;
        presenter.on_travel(app.world(), None)?;
        let audio = match Audio::new_default() {
            Ok(audio) => Some(audio),
            Err(error) => {
                platform::log_error(&format!("audio unavailable; continuing silently: {error}"));
                None
            }
        };
        let jobs = app.jobs().clone();
        Ok(Self {
            app,
            scene_content,
            live_scene: LiveScene::new(),
            presenter,
            session,
            pending_command: None,
            audio,
            audio_failure_logged: false,
            settings,
            tick_rate: config.tick_rate,
            gfx: None,
            asset_store: AssetStore::new(),
            asset_uploader: AssetUploader::new(),
            jobs,
            extractor: Extractor::with_capacity(capacity),
            front: RenderFrame::with_capacity(capacity),
            back: RenderFrame::with_capacity(capacity),
            extracted: false,
            look: ViewAngles::new(0.0, 0.0)?,
            sensitivity: LookSensitivity::default(),
            error: None,
            failed: false,
            governor: None,
            drawn_touch_regions: Vec::with_capacity(platform::MAX_TOUCH_REGIONS),
            telemetry: Telemetry::new(),
            presentation: PresentationBinding::default(),
            sim_ms: 0.0,
            error_banner: ErrorBanner::default(),
            error_font: None,
            last_logged_error: None,
        })
    }

    /// Turns on dynamic resolution: after each rendered frame the governor sees the frame's wall
    /// time and may change the render scale. The scale starts at the settings' `render_scale`,
    /// which must be a whole step of the governor inside its range.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Render`] when the governor rejects the start scale.
    pub fn with_governor(mut self, config: GovernorConfig) -> Result<Self, AppError> {
        self.governor = Some(Governor::new(config, self.settings.render_scale)?);
        Ok(self)
    }

    /// The app this client runs.
    #[must_use]
    pub const fn app(&self) -> &App<G> {
        &self.app
    }

    /// Rebuilds the game world for new settings, then resets render history and presenter caches.
    pub fn travel(&mut self, settings: &G::Settings) -> Result<(), AppError> {
        if let Session::Online(session) = &mut self.session {
            session.prepare_travel(self.app.game(), settings)?;
        }
        self.app.travel(settings)?;
        if matches!(self.session, Session::Standalone) {
            let local_name = engine_sessions::PlayerName::new("Local")?;
            self.app.join_player(PlayerId::LOCAL, &local_name)?;
        } else if let Session::Online(session) = &mut self.session {
            session.set_travel_epoch(self.app.travel_epoch());
        }
        self.ensure_frame_capacity()?;
        self.pending_command = None;
        self.extractor.reset();
        self.front.clear();
        self.back.clear();
        self.extracted = false;
        self.live_scene.reset(&mut self.asset_store)?;
        self.presenter
            .on_travel(self.app.world(), self.gfx.as_mut())
    }

    /// The presenter, for reading its state after a run.
    #[must_use]
    pub const fn presenter(&self) -> &P {
        &self.presenter
    }

    /// The presenter, for controlled client-side diagnostics and tools.
    pub fn presenter_mut(&mut self) -> &mut P {
        &mut self.presenter
    }

    /// The newest extracted frame: the transforms of the last extracted tick.
    #[must_use]
    pub const fn frame(&self) -> &RenderFrame {
        &self.front
    }

    /// The frame telemetry over the last frames: fps, frame-time percentiles, the display
    /// rate estimate and the simulation cost. The same report `Presenter::observe` receives.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::TelemetryIndex`] only on an internal bug.
    pub fn report(&mut self) -> Result<FrameReport, AppError> {
        self.telemetry.snapshot()
    }

    /// The fatal error raised inside a platform callback, if any. Taking it leaves `None`; the
    /// client stays stopped either way.
    pub fn take_error(&mut self) -> Option<AppError> {
        self.error.take()
    }

    /// Whether the client loop can continue after the most recent frame.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        !self.failed
    }

    /// Current recoverable error-banner text, if one is visible.
    #[must_use]
    pub fn error_banner_message(&self) -> Option<&str> {
        self.error_banner.message()
    }

    /// Mounts the game's validated content pack before its first rendered frame.
    pub fn mount_content(&mut self, content: ContentPack) -> Result<(), AppError> {
        self.asset_store.mount_content(content, &self.jobs)?;
        Ok(())
    }

    /// Runs the fixed ticks that `dt` seconds owe, extracts when a tick ran (or on the very
    /// first call), and returns the interpolation weight for drawing. It also times itself for
    /// the telemetry's simulation cost.
    ///
    /// `dt` is the wall time since the last call in seconds. It must be finite and not negative.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ClockRange`] for a `dt` that is negative, NaN, infinite or too large
    /// for the clock; any error from [`App::advance`] or [`Extractor::extract`]. On error the
    /// front frame keeps its last good extract.
    pub fn simulate(&mut self, dt: f32) -> Result<f32, AppError> {
        let started = Instant::now();
        let alpha = self.step(dt)?;
        self.sim_ms = started.elapsed().as_secs_f32() * 1000.0;
        Ok(alpha)
    }

    /// The body of [`ClientApp::simulate`], without the timing.
    fn step(&mut self, dt: f32) -> Result<f32, AppError> {
        let frame_nanos = frame_nanos(dt)?;
        let mut assets = ClientAssets {
            store: &mut self.asset_store,
            uploader: &self.asset_uploader,
            pool: &self.jobs,
        };
        let presenter = &mut self.presenter;
        let command = self.pending_command.as_ref();
        let advance = match &mut self.session {
            Session::Standalone => self.app.advance_with(frame_nanos, |game, world| {
                presenter.before_ticks(world, &mut assets)?;
                if let Some(command) = command {
                    game.command(world, PlayerId::LOCAL, command)?;
                }
                Ok(())
            })?,
            Session::Online(session) => {
                self.app.advance_external(frame_nanos, |world, _tick| {
                    presenter.before_ticks(world, &mut assets)?;
                    if let Some(command) = command {
                        session.tick(world, command.clone())?;
                    }
                    Ok(())
                })?
            }
        };
        let travel = match &mut self.session {
            Session::Standalone => None,
            Session::Online(session) => session.take_pending_travel(),
        };
        if let Some(settings) = travel {
            self.apply_requested_travel(&settings);
        }
        if advance.ticks > 0 || !self.extracted {
            self.extractor
                .extract(self.app.world(), self.app.tick(), &mut self.back)?;
            std::mem::swap(&mut self.front, &mut self.back);
            self.extracted = true;
        }
        advance.alpha(self.tick_rate)
    }

    /// Polls the engine-owned level watcher once at the desktop debug frame boundary.
    fn poll_live_scene(&mut self) -> Result<(), AppError> {
        if cfg!(all(debug_assertions, not(target_os = "android"))) {
            let accepted = self.live_scene.poll(
                self.app.world_mut(),
                &self.scene_content,
                &mut self.asset_store,
                &self.asset_uploader,
                &self.jobs,
            )?;
            if let Some(level) = accepted {
                self.app
                    .scene_reloaded(&level, self.asset_store.content_pack())?;
                self.ensure_frame_capacity()?;
            }
        }
        self.presenter.before_frame(self.app.world_mut())?;
        Ok(())
    }

    /// Resizes extraction storage after a successful world load, never during a tick or frame.
    fn ensure_frame_capacity(&mut self) -> Result<(), AppError> {
        let capacity = entity_capacity(self.app.world(), self.app.game().dynamic_entity_budget())?;
        if capacity > self.extractor.capacity() {
            self.extractor = Extractor::with_capacity(capacity);
            self.front = RenderFrame::with_capacity(capacity);
            self.back = RenderFrame::with_capacity(capacity);
            self.extracted = false;
        }
        Ok(())
    }

    /// Records a fatal error and stops the client.
    fn fail(&mut self, error: AppError) {
        self.failed = true;
        self.error = Some(error);
    }

    fn apply_requested_travel(&mut self, settings: &G::Settings) {
        if let Err(error) = self.travel(settings) {
            if let Err(recovery_error) = self.presenter.travel_failed(&error) {
                self.report_travel_error(recovery_error);
            }
            self.report_travel_error(error);
        }
    }

    fn report_travel_error(&mut self, error: AppError) {
        let now = Instant::now();
        let message = error.to_string();
        let should_log = self
            .last_logged_error
            .as_ref()
            .is_none_or(|(last, logged)| {
                last != &message || now.duration_since(*logged) >= ERROR_LOG_INTERVAL
            });
        if should_log {
            platform::log_error(&message);
            self.last_logged_error = Some((message.clone(), now));
        }
        self.error_banner.show(message, now);
    }

    /// Builds the renderer and hands it to the presenter.
    fn try_init(
        &mut self,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Result<(), AppError> {
        let mut gfx = Gfx::new(gpu, format, size, self.settings)?;
        let mut assets = ClientAssets {
            store: &mut self.asset_store,
            uploader: &self.asset_uploader,
            pool: &self.jobs,
        };
        self.presenter.setup(&mut gfx, &mut assets)?;
        self.live_scene.setup(&mut gfx)?;
        self.error_font = Some(
            gfx.ui
                .atlas
                .add_font(include_bytes!("../../render/fonts/Roboto-Medium.ttf"))?,
        );
        self.gfx = Some(gfx);
        Ok(())
    }

    /// Turns the view by this frame's look input: mouse motion while the window holds the grab,
    /// and the right-half look touch at any time (touch needs no grab).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NonFiniteAngle`] when the scaled motion overflows.
    fn turn_view(&mut self, input: &Input) -> Result<(), AppError> {
        let scale = self.presenter.look_scale();
        if !scale.is_finite() || scale <= 0.0 || scale > 1.0 {
            return Err(AppError::InvalidLookScale(scale));
        }
        let sensitivity = self
            .presenter
            .look_sensitivity()
            .unwrap_or(self.sensitivity);
        if input.grabbed {
            let mouse = sensitivity.delta(input.mouse_delta * scale)?;
            self.look = self.look.turned(mouse);
        }
        let touch = sensitivity.delta(input.touch_look * scale)?;
        self.look = self.look.turned(touch);
        Ok(())
    }

    /// The input step sets mouse grab, turns the view, and runs the presenter's command. UI regions
    /// are installed only after a successful prior draw, so a first-frame touch intentionally hits
    /// nothing. [`PlatformApp::frame`] runs this before drawing.
    ///
    /// # Errors
    ///
    /// Returns errors from view turning or command conversion.
    pub fn prepare_input(&mut self, input: &mut Input) -> Result<(), AppError> {
        input.want_grab = self.presenter.grab_mouse();
        self.turn_view(input)?;
        self.pending_command = Some(self.presenter.command(input, self.look)?);
        if let Some(settings) = self.presenter.take_travel() {
            if matches!(self.session, Session::Online(_)) {
                return Err(AppError::TravelRequestedOnline);
            }
            self.apply_requested_travel(&settings);
        }
        Ok(())
    }

    /// Records this frame in the telemetry and hands the report to the presenter. Called once per
    /// rendered frame, after `simulate`, so the report covers the frame just simulated.
    fn observe_frame(
        &mut self,
        dt: f32,
        render_scale: f32,
        gpu: Option<&GpuTimes>,
    ) -> Result<(), AppError> {
        self.telemetry.record(dt * 1000.0, self.sim_ms)?;
        let report = self.telemetry.snapshot()?;
        self.presenter
            .observe(&report, render_scale, gpu, self.presentation);
        Ok(())
    }

    /// Present intent for the active surface session (from [`PlatformApp::on_presentation_bound`]).
    #[must_use]
    pub const fn presentation_binding(&self) -> PresentationBinding {
        self.presentation
    }

    /// One rendered frame, in the order of the module header.
    fn run_frame(
        &mut self,
        target: &wgpu::TextureView,
        input: &mut Input,
        dt: f32,
    ) -> Result<(), FrameFailure> {
        self.prepare_input(input).map_err(FrameFailure::Skip)?;
        self.poll_live_scene().map_err(FrameFailure::Skip)?;
        let alpha = self.simulate(dt).map_err(|error| {
            if matches!(self.session, Session::Online(_))
                || matches!(&error, AppError::ExtractCapacity { .. })
            {
                FrameFailure::Skip(error)
            } else {
                FrameFailure::Stop(error)
            }
        })?;
        let camera = self
            .presenter
            .camera(&self.front, alpha, self.look)
            .map_err(FrameFailure::Skip)?;
        if let Some(audio) = &mut self.audio {
            audio.set_listener(camera.pos, listener_rotation(&camera));
            if let Err(error) = self.presenter.play_audio(self.app.world(), audio)
                && !self.audio_failure_logged
            {
                platform::log_error(&format!(
                    "audio playback failed; continuing silently: {error}"
                ));
                self.audio_failure_logged = true;
            }
        }
        let (render_scale, gpu_times) = self
            .gfx
            .as_ref()
            .map(|gfx| (gfx.render_scale(), gfx.gpu_times()))
            .ok_or(FrameFailure::Stop(AppError::ClientNotReady))?;
        self.observe_frame(dt, render_scale, gpu_times.as_ref())
            .map_err(FrameFailure::Skip)?;
        let gfx = self
            .gfx
            .as_mut()
            .ok_or(FrameFailure::Stop(AppError::ClientNotReady))?;
        gfx.begin_frame(camera);
        {
            let mut assets = ClientAssets {
                store: &mut self.asset_store,
                uploader: &self.asset_uploader,
                pool: &self.jobs,
            };
            self.presenter
                .load_frame_assets(&self.front, &self.scene_content, &mut assets)
                .map_err(FrameFailure::Skip)?;
        }
        self.asset_store
            .apply_loaded(&self.jobs, Instant::now())
            .map_err(AppError::from)
            .map_err(FrameFailure::Skip)?;
        let budget = gfx.tier().upload_budget_bytes();
        let uploaded = self
            .asset_uploader
            .update(&mut self.asset_store, gfx, &self.jobs, budget)
            .map_err(FrameFailure::Skip)?;
        gfx.note_asset_upload_bytes(uploaded);
        let mut assets = ClientAssets {
            store: &mut self.asset_store,
            uploader: &self.asset_uploader,
            pool: &self.jobs,
        };
        self.presenter
            .draw(&self.front, alpha, gfx, &mut assets)
            .map_err(FrameFailure::Skip)?;
        self.drawn_touch_regions.clear();
        let presentation_error =
            match self
                .presenter
                .draw_ui(gfx, &mut self.drawn_touch_regions, &mut assets)
            {
                Ok(()) if input.set_touch_regions(&self.drawn_touch_regions) => None,
                Ok(()) => Some(AppError::Ui(engine_ui::UiError::Layout(
                    "drawn touch region list is invalid".to_owned(),
                ))),
                Err(error) => Some(error),
            };
        if presentation_error.is_some() {
            self.drawn_touch_regions.clear();
        }
        let _ = gfx;
        if let Some(error) = presentation_error {
            self.report_presentation_error(error);
        }
        let gfx = self
            .gfx
            .as_mut()
            .ok_or(FrameFailure::Stop(AppError::ClientNotReady))?;
        let now = Instant::now();
        self.error_banner.clear_expired(now);
        if let Some(font) = self.error_font {
            let screen_width = gfx.ui.size.x;
            self.live_scene.draw(&mut gfx.ui, screen_width);
            self.error_banner.draw(&mut gfx.ui, screen_width, font);
        }
        gfx.render(target, dt);
        if let Some(governor) = self.governor.as_mut()
            && let Some(scale) = governor
                .observe(dt * 1000.0)
                .map_err(AppError::from)
                .map_err(FrameFailure::Skip)?
        {
            gfx.set_render_scale(scale);
        }
        Ok(())
    }

    /// Reports a presentation failure without affecting simulation recovery.
    fn report_presentation_error(&mut self, error: AppError) {
        self.telemetry.record_presentation_error();
        let now = Instant::now();
        let message = error.to_string();
        let should_log = self
            .last_logged_error
            .as_ref()
            .is_none_or(|(last, _)| last != &message);
        if should_log {
            platform::log_error(&message);
            self.last_logged_error = Some((message.clone(), now));
        }
        self.error_banner.show(message, now);
    }
}

fn listener_rotation(camera: &Camera) -> glam::Quat {
    glam::Quat::from_rotation_y(camera.yaw) * glam::Quat::from_rotation_x(camera.pitch)
}

impl<G: Game + 'static, P: Presenter<Command = G::Command, Settings = G::Settings>> PlatformApp
    for ClientApp<G, P>
{
    fn init(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, size: (u32, u32)) {
        if self.failed {
            return;
        }
        if let Err(error) = self.try_init(gpu, format, size) {
            self.fail(error);
        }
    }

    fn resize(&mut self, size: (u32, u32)) {
        if self.failed {
            return;
        }
        match self.gfx.as_mut() {
            Some(gfx) => gfx.resize(size),
            None => self.fail(AppError::ClientNotReady),
        }
    }

    fn suspend(&mut self) {
        self.gfx = None;
        self.error_font = None;
    }

    fn resume(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, size: (u32, u32)) {
        if self.failed {
            return;
        }
        if let Err(error) = self.try_init(gpu, format, size) {
            self.fail(error);
        }
    }

    fn on_presentation_bound(&mut self, binding: PresentationBinding) {
        self.presentation = binding;
    }

    fn frame(&mut self, target: &wgpu::TextureView, input: &mut Input, dt: f32) {
        if self.failed {
            input.quit = true;
            return;
        }
        if let Err(failure) = self.run_frame(target, input, dt) {
            match failure {
                FrameFailure::Stop(error) => self.fail(error),
                FrameFailure::Skip(error) => self.report_presentation_error(error),
            }
            if self.failed {
                input.quit = true;
            }
        }
    }
}

/// Opens a window of `size` physical pixels titled `title` and runs `client` until the window
/// closes or a fatal error occurs. Recoverable frame errors are shown in the engine-owned banner.
///
/// # Errors
///
/// Returns [`AppError::Platform`] when the window or GPU cannot be created, or the first error
/// raised during startup or standalone simulation.
pub fn run_client<G: Game + 'static, P: Presenter<Command = G::Command, Settings = G::Settings>>(
    title: &str,
    size: (u32, u32),
    client: &mut ClientApp<G, P>,
) -> Result<(), AppError> {
    platform::window::run(title, size, &mut *client)?;
    finish(client)
}

/// Runs `client` in the window of a NativeActivity bound to `android`. Same contract as
/// [`run_client`].
///
/// # Errors
///
/// Returns [`AppError::Platform`] when the window or GPU cannot be created, or the first error
/// raised inside a frame callback.
#[cfg(target_os = "android")]
pub fn run_client_android<
    G: Game + 'static,
    P: Presenter<Command = G::Command, Settings = G::Settings>,
>(
    android: platform::AndroidApp,
    title: &str,
    size: (u32, u32),
    client: &mut ClientApp<G, P>,
) -> Result<(), AppError> {
    platform::window::run_android(android, title, size, &mut *client)?;
    finish(client)
}

/// Hands back the first error a frame callback recorded, once the window loop has ended.
fn finish<G: Game + 'static, P: Presenter<Command = G::Command, Settings = G::Settings>>(
    client: &mut ClientApp<G, P>,
) -> Result<(), AppError> {
    match client.take_error() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Converts a frame time in seconds to whole nanoseconds for the clock.
///
/// # Errors
///
/// Returns [`AppError::ClockRange`] for a negative, NaN, infinite or unrepresentable value.
fn frame_nanos(dt: f32) -> Result<u64, AppError> {
    let duration = Duration::try_from_secs_f32(dt).map_err(|_| AppError::ClockRange)?;
    u64::try_from(duration.as_nanos()).map_err(|_| AppError::ClockRange)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use engine_core::StateHasher;
    use engine_world::{NetId, Resource, Schedule};
    use glam::Vec2;
    use serde::{Deserialize, Serialize};

    #[test]
    fn listener_faces_where_the_camera_looks_in_kiras_frame() {
        // Kira's listener looks down its local -Z and puts the right ear on its local +X.
        for (yaw, pitch) in [(0.0, 0.0), (std::f32::consts::FRAC_PI_2, 0.0), (0.7, -0.4)] {
            let camera = Camera {
                yaw,
                pitch,
                ..Camera::default()
            };
            let rotation = listener_rotation(&camera);
            let forward = camera.forward();
            let right = forward.cross(glam::Vec3::Y).normalize();
            assert!((rotation * glam::Vec3::NEG_Z - forward).length() < 1.0e-5);
            assert!((rotation * glam::Vec3::X - right).length() < 1.0e-5);
        }
    }

    struct DrawFailures {
        calls: std::cell::Cell<u32>,
        commands: std::cell::Cell<u32>,
        fail_always: bool,
        fail_ui_always: bool,
        successful_draws: std::cell::RefCell<Vec<u32>>,
    }

    impl Presenter for DrawFailures {
        type Command = ();
        type Settings = ();

        fn setup(
            &mut self,
            _gfx: &mut Gfx,
            _assets: &mut ClientAssets<'_>,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn command(
            &mut self,
            _input: &Input,
            _look: ViewAngles,
        ) -> Result<Self::Command, AppError> {
            self.commands.set(self.commands.get().saturating_add(1));
            Ok(())
        }

        fn grab_mouse(&self) -> bool {
            false
        }

        fn camera(
            &self,
            _frame: &RenderFrame,
            _alpha: f32,
            _look: ViewAngles,
        ) -> Result<Camera, AppError> {
            Ok(Camera::default())
        }

        fn draw(
            &mut self,
            _frame: &RenderFrame,
            _alpha: f32,
            _gfx: &mut Gfx,
            _assets: &mut ClientAssets<'_>,
        ) -> Result<(), AppError> {
            let call = self.calls.get() + 1;
            self.calls.set(call);
            if self.fail_always || call == 3 || call == 4 {
                return Err(AppError::ClockRange);
            }
            self.successful_draws.borrow_mut().push(call);
            Ok(())
        }

        fn draw_ui(
            &mut self,
            _gfx: &mut Gfx,
            _regions: &mut Vec<TouchRegion>,
            _assets: &mut ClientAssets<'_>,
        ) -> Result<(), AppError> {
            if self.fail_ui_always {
                return Err(AppError::Ui(engine_ui::UiError::Layout(
                    "forced HUD layout failure".to_owned(),
                )));
            }
            Ok(())
        }
    }

    /// A game with no systems: the look tests never tick.
    struct Empty;

    impl Game for Empty {
        fn dynamic_entity_budget(&self) -> usize {
            0
        }

        type Command = ();
        type Settings = ();

        fn info(&self) -> crate::GameInfo {
            crate::GameInfo {
                name: "empty",
                protocol_id: 1,
                protocol_version: 1,
            }
        }

        fn setup(
            &self,
            _world: &mut World,
            _schedule: &mut Schedule,
            _: &Self::Settings,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn join(
            &self,
            world: &mut World,
            _: PlayerId,
            _: &engine_sessions::PlayerName,
        ) -> Result<NetId, AppError> {
            Ok(world.spawn()?)
        }

        fn leave(&self, _: &mut World, _: PlayerId) -> Result<(), AppError> {
            Ok(())
        }

        fn command(&self, _: &mut World, _: PlayerId, _: &()) -> Result<(), AppError> {
            Ok(())
        }
    }

    /// A presenter that does nothing; `turn_view` never reaches the presenter.
    struct Quiet;

    impl Presenter for Quiet {
        type Command = ();
        type Settings = ();

        fn setup(
            &mut self,
            _gfx: &mut Gfx,
            _assets: &mut ClientAssets<'_>,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn command(
            &mut self,
            _input: &Input,
            _look: ViewAngles,
        ) -> Result<Self::Command, AppError> {
            Ok(())
        }

        fn grab_mouse(&self) -> bool {
            false
        }

        fn camera(
            &self,
            _frame: &RenderFrame,
            _alpha: f32,
            _look: ViewAngles,
        ) -> Result<Camera, AppError> {
            Err(AppError::ClientNotReady)
        }

        fn draw(
            &mut self,
            _frame: &RenderFrame,
            _alpha: f32,
            _gfx: &mut Gfx,
            _assets: &mut ClientAssets<'_>,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    /// A presenter that records every frame report it is handed.
    #[derive(Default)]
    struct Recorder {
        reports: Vec<FrameReport>,
        scales: Vec<f32>,
        look_scale: Option<f32>,
    }

    impl Presenter for Recorder {
        type Command = ();
        type Settings = ();

        fn setup(
            &mut self,
            _gfx: &mut Gfx,
            _assets: &mut ClientAssets<'_>,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn command(
            &mut self,
            _input: &Input,
            _look: ViewAngles,
        ) -> Result<Self::Command, AppError> {
            Ok(())
        }

        fn grab_mouse(&self) -> bool {
            false
        }

        fn look_scale(&self) -> f32 {
            self.look_scale.unwrap_or(1.0)
        }

        fn observe(
            &mut self,
            report: &FrameReport,
            render_scale: f32,
            _gpu: Option<&GpuTimes>,
            _presentation: PresentationBinding,
        ) {
            self.reports.push(*report);
            self.scales.push(render_scale);
        }

        fn camera(
            &self,
            _frame: &RenderFrame,
            _alpha: f32,
            _look: ViewAngles,
        ) -> Result<Camera, AppError> {
            Err(AppError::ClientNotReady)
        }

        fn draw(
            &mut self,
            _frame: &RenderFrame,
            _alpha: f32,
            _gfx: &mut Gfx,
            _assets: &mut ClientAssets<'_>,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    /// A client with the default sensitivity and no GPU.
    fn client() -> ClientApp<Empty, Quiet> {
        client_with(Quiet)
    }

    /// A client with the default sensitivity, no GPU and the given presenter.
    fn client_with<P: Presenter<Command = (), Settings = ()>>(presenter: P) -> ClientApp<Empty, P> {
        let config = AppConfig {
            tick_rate: crate::TickRate::new(60).expect("valid rate"),
            seed: 1,
            max_catch_up_ticks: 4,
            job_workers: Some(0),
        };
        let profile = DeviceProfile {
            class: platform::DeviceClass::Desktop,
            touch: false,
            cpu: platform::CpuProfile {
                logical_cores: std::num::NonZeroUsize::new(1).expect("non-zero"),
                performance_cores: std::num::NonZeroUsize::new(1).expect("non-zero"),
            },
            memory_bytes: None,
            gpu: None,
        };
        ClientApp::new(
            Empty,
            &(),
            presenter,
            &config,
            &profile,
            Settings::default(),
        )
        .expect("client")
    }

    fn draw_failures(fail_always: bool) -> DrawFailures {
        DrawFailures {
            calls: std::cell::Cell::new(0),
            commands: std::cell::Cell::new(0),
            fail_always,
            fail_ui_always: false,
            successful_draws: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn gpu_unavailable(error: &platform::PlatformError) -> bool {
        if matches!(error, platform::PlatformError::NoAdapter(_)) {
            println!("skipping client recovery test: no GPU adapter is available");
            true
        } else {
            false
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct TravelSettings(u32);

    struct TravelValue(u32);

    impl Resource for TravelValue {
        fn hash_into(&self, hasher: &mut StateHasher) {
            hasher.write_u32(self.0);
        }
    }

    struct TravelGame;

    impl Game for TravelGame {
        fn dynamic_entity_budget(&self) -> usize {
            0
        }

        type Command = ();
        type Settings = TravelSettings;

        fn info(&self) -> crate::GameInfo {
            crate::GameInfo {
                name: "travel-test",
                protocol_id: 99,
                protocol_version: 1,
            }
        }

        fn setup(
            &self,
            world: &mut World,
            _schedule: &mut Schedule,
            settings: &Self::Settings,
        ) -> Result<(), AppError> {
            if settings.0 == 3 {
                return Err(AppError::ClockRange);
            }
            world.insert_resource(TravelValue(settings.0))?;
            Ok(())
        }

        fn join(
            &self,
            world: &mut World,
            _player: PlayerId,
            _name: &engine_sessions::PlayerName,
        ) -> Result<NetId, AppError> {
            Ok(world.spawn()?)
        }

        fn leave(&self, _world: &mut World, _player: PlayerId) -> Result<(), AppError> {
            Ok(())
        }

        fn command(
            &self,
            _world: &mut World,
            _player: PlayerId,
            _command: &Self::Command,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    struct TravelPresenter {
        request: Option<TravelSettings>,
        travel_calls: u32,
    }

    impl Presenter for TravelPresenter {
        type Command = ();
        type Settings = TravelSettings;

        fn setup(
            &mut self,
            _gfx: &mut Gfx,
            _assets: &mut ClientAssets<'_>,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn command(
            &mut self,
            _input: &Input,
            _look: ViewAngles,
        ) -> Result<Self::Command, AppError> {
            Ok(())
        }

        fn take_travel(&mut self) -> Option<Self::Settings> {
            self.request.take()
        }

        fn on_travel(&mut self, _world: &World, _gfx: Option<&mut Gfx>) -> Result<(), AppError> {
            self.travel_calls = self.travel_calls.saturating_add(1);
            Ok(())
        }

        fn grab_mouse(&self) -> bool {
            false
        }

        fn camera(
            &self,
            _frame: &RenderFrame,
            _alpha: f32,
            _look: ViewAngles,
        ) -> Result<Camera, AppError> {
            Ok(Camera::default())
        }

        fn draw(
            &mut self,
            _frame: &RenderFrame,
            _alpha: f32,
            _gfx: &mut Gfx,
            _assets: &mut ClientAssets<'_>,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    struct TestOnlineSession;

    impl ClientSession<TravelGame> for TestOnlineSession {
        fn tick(&mut self, _world: &mut World, _command: ()) -> Result<(), AppError> {
            Ok(())
        }

        fn take_pending_travel(&mut self) -> Option<TravelSettings> {
            None
        }

        fn prepare_travel(
            &mut self,
            _game: &TravelGame,
            _settings: &TravelSettings,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn set_travel_epoch(&mut self, _epoch: u32) {}
    }

    fn travel_client(
        presenter: TravelPresenter,
        session: Session<TravelGame>,
    ) -> Result<ClientApp<TravelGame, TravelPresenter>, AppError> {
        let config = AppConfig {
            tick_rate: crate::TickRate::new(60)?,
            seed: 1,
            max_catch_up_ticks: 4,
            job_workers: Some(0),
        };
        let profile = DeviceProfile {
            class: platform::DeviceClass::Desktop,
            touch: false,
            cpu: platform::CpuProfile {
                logical_cores: std::num::NonZeroUsize::MIN,
                performance_cores: std::num::NonZeroUsize::MIN,
            },
            memory_bytes: None,
            gpu: None,
        };
        ClientApp::build(
            TravelGame,
            &TravelSettings(1),
            presenter,
            &config,
            &profile,
            Settings::default(),
            session,
        )
    }

    #[test]
    fn standalone_presenter_travel_rebuilds_world_before_ticks() -> Result<(), AppError> {
        let presenter = TravelPresenter {
            request: Some(TravelSettings(2)),
            travel_calls: 0,
        };
        let mut client = travel_client(presenter, Session::Standalone)?;
        let mut input = Input::default();

        client.prepare_input(&mut input)?;

        assert_eq!(client.app.travel_epoch(), 1);
        assert_eq!(client.app.world().resource::<TravelValue>()?.0, 2);
        assert_eq!(client.presenter.travel_calls, 2);
        Ok(())
    }

    #[test]
    fn presenter_without_a_request_does_not_travel() -> Result<(), AppError> {
        let presenter = TravelPresenter {
            request: None,
            travel_calls: 0,
        };
        let mut client = travel_client(presenter, Session::Standalone)?;
        let mut input = Input::default();

        client.prepare_input(&mut input)?;

        assert_eq!(client.app.travel_epoch(), 0);
        assert_eq!(client.app.world().resource::<TravelValue>()?.0, 1);
        assert_eq!(client.presenter.travel_calls, 1);
        Ok(())
    }

    #[test]
    fn failed_presenter_travel_keeps_the_world_reports_an_error_and_keeps_running()
    -> Result<(), AppError> {
        let presenter = TravelPresenter {
            request: Some(TravelSettings(3)),
            travel_calls: 0,
        };
        let mut client = travel_client(presenter, Session::Standalone)?;
        let mut input = Input::default();

        client.prepare_input(&mut input)?;

        assert_eq!(client.app.travel_epoch(), 0);
        assert_eq!(client.app.world().resource::<TravelValue>()?.0, 1);
        assert!(client.is_running());
        assert!(
            client
                .error_banner
                .message()
                .is_some_and(|message| { message.contains("clock remainder out of range") })
        );
        Ok(())
    }

    #[test]
    fn presenter_default_does_not_request_travel() -> Result<(), AppError> {
        let mut client = client();
        let mut input = Input::default();

        client.prepare_input(&mut input)?;

        assert_eq!(client.app.travel_epoch(), 0);
        Ok(())
    }

    #[test]
    fn online_presenter_travel_returns_the_typed_error() -> Result<(), AppError> {
        let presenter = TravelPresenter {
            request: Some(TravelSettings(2)),
            travel_calls: 0,
        };
        let session = Session::Online(Box::new(TestOnlineSession));
        let mut client = travel_client(presenter, session)?;
        let mut input = Input::default();

        assert!(matches!(
            client.prepare_input(&mut input),
            Err(AppError::TravelRequestedOnline)
        ));
        assert_eq!(client.app.travel_epoch(), 0);
        Ok(())
    }

    #[test]
    fn failed_draws_keep_the_renderer_and_the_next_frame_recovers() -> Result<(), AppError> {
        let presenter = draw_failures(false);
        let mut client = client_with(presenter);
        if let Err(error) = platform::headless::run(&mut client, (8, 8), 5, None) {
            if gpu_unavailable(&error) {
                return Ok(());
            }
            return Err(error.into());
        }
        let presenter = client.presenter();
        assert_eq!(*presenter.successful_draws.borrow(), vec![1, 2, 5]);
        assert_eq!(
            client.error_banner.message(),
            Some("clock remainder out of range")
        );
        assert!(!client.failed);
        Ok(())
    }

    #[test]
    fn three_hundred_failed_ui_draws_keep_the_client_running() -> Result<(), AppError> {
        let mut presenter = draw_failures(false);
        presenter.fail_ui_always = true;
        let mut client = client_with(presenter);
        if let Err(error) = platform::headless::run(&mut client, (8, 8), 300, None) {
            if gpu_unavailable(&error) {
                return Ok(());
            }
            return Err(error.into());
        }
        assert!(client.is_running());
        assert_eq!(client.presenter().calls.get(), 300);
        assert_eq!(client.presenter().commands.get(), 300);
        assert_eq!(
            client.error_banner.message(),
            Some("ui error: ui layout failed: forced HUD layout failure")
        );
        let mut input = Input::default();
        client.prepare_input(&mut input)?;
        assert_eq!(client.presenter().commands.get(), 301);
        Ok(())
    }

    #[test]
    fn three_hundred_presentation_draw_failures_keep_the_client_running() -> Result<(), AppError> {
        let presenter = draw_failures(true);
        let mut client = client_with(presenter);
        if let Err(error) = platform::headless::run(&mut client, (8, 8), 300, None) {
            if gpu_unavailable(&error) {
                return Ok(());
            }
            return Err(error.into());
        }
        assert!(client.is_running());
        assert_eq!(
            client.error_banner.message(),
            Some("clock remainder out of range")
        );
        let mut input = Input::default();
        client.prepare_input(&mut input)?;
        assert!(!client.failed);
        Ok(())
    }

    #[test]
    fn the_presenter_observes_each_simulated_frame_at_the_scripted_rate() -> Result<(), AppError> {
        let mut client = client_with(Recorder::default());
        let dt = 1.0 / 120.0;
        for _ in 0..10 {
            client.simulate(dt)?;
            client.observe_frame(dt, 0.75, None)?;
        }
        let presenter = client.presenter();
        assert_eq!(presenter.reports.len(), 10, "one observation per frame");
        assert!(presenter.scales.iter().all(|scale| *scale == 0.75));
        let last = client.report()?;
        assert!((last.fps - 120.0).abs() < 0.01, "fps {}", last.fps);
        assert_eq!(last.refresh_hz_estimate, Some(120));
        assert!(last.sim_ms_avg >= 0.0 && last.sim_ms_avg.is_finite());
        Ok(())
    }

    #[test]
    fn touch_look_turns_the_view_without_a_grab() {
        let mut client = client();
        let mut input = Input::default();
        input.touch_look = Vec2::new(100.0, 0.0);
        client.turn_view(&input).expect("finite motion");
        // Moving right turns right: yaw goes down by 100 px times the default sensitivity.
        assert!((client.look.yaw() + 0.4).abs() < 1e-5);
        assert!(client.look.pitch().abs() < 1e-6);
    }

    #[test]
    fn presenter_scale_halves_mouse_and_touch_motion() {
        let presenter = Recorder {
            look_scale: Some(0.5),
            ..Recorder::default()
        };
        let mut client = client_with(presenter);
        let mut input = Input::default();
        input.grabbed = true;
        input.mouse_delta = Vec2::new(100.0, 0.0);
        input.touch_look = Vec2::new(100.0, 0.0);
        client.turn_view(&input).expect("valid presenter scale");
        assert!((client.look.yaw() + 0.4).abs() < 1e-5);
    }

    #[test]
    fn invalid_presenter_look_scales_are_typed_errors() {
        for scale in [0.0, -0.1, 1.1, f32::NAN, f32::INFINITY] {
            let presenter = Recorder {
                look_scale: Some(scale),
                ..Recorder::default()
            };
            let mut client = client_with(presenter);
            assert!(matches!(
                client.turn_view(&Input::default()),
                Err(AppError::InvalidLookScale(_))
            ));
        }
    }

    #[test]
    fn mouse_look_needs_the_grab() {
        let mut client = client();
        let mut input = Input::default();
        input.mouse_delta = Vec2::new(100.0, 0.0);
        client.turn_view(&input).expect("finite motion");
        assert_eq!(client.look, ViewAngles::ZERO);
        input.grabbed = true;
        client.turn_view(&input).expect("finite motion");
        assert!((client.look.yaw() + 0.4).abs() < 1e-5);
    }

    #[test]
    fn mouse_and_touch_look_add_up_in_one_frame() {
        let mut client = client();
        let mut input = Input::default();
        input.grabbed = true;
        input.mouse_delta = Vec2::new(0.0, -40.0);
        input.touch_look = Vec2::new(0.0, -40.0);
        client.turn_view(&input).expect("finite motion");
        assert!((client.look.pitch() - 0.32).abs() < 1e-5);
    }

    #[test]
    fn overflowing_touch_look_is_an_error() {
        let mut client = client();
        client.sensitivity = crate::LookSensitivity::new(f32::MAX).expect("valid");
        let mut input = Input::default();
        input.touch_look = Vec2::new(10.0, 0.0);
        assert!(matches!(
            client.turn_view(&input),
            Err(AppError::NonFiniteAngle)
        ));
    }

    #[test]
    fn presentation_errors_reach_the_banner_and_telemetry() {
        let mut client = client();
        client.report_presentation_error(AppError::ExtractCapacity {
            capacity: 4,
            needed: 5,
        });
        assert_eq!(
            client.error_banner_message(),
            Some("extract needs room for 5 transforms but the capacity is 4")
        );
        assert_eq!(
            client
                .report()
                .expect("telemetry snapshot")
                .presentation_errors,
            1
        );
    }

    #[test]
    fn frame_time_converts_to_whole_nanoseconds() {
        assert_eq!(frame_nanos(0.0).expect("zero is valid"), 0);
        assert_eq!(frame_nanos(0.5).expect("half a second"), 500_000_000);
    }

    #[test]
    fn frame_time_rejects_negative_and_non_finite_values() {
        for bad in [-0.001, f32::NAN, f32::INFINITY] {
            assert!(
                matches!(frame_nanos(bad), Err(AppError::ClockRange)),
                "{bad} must be rejected"
            );
        }
    }
}
