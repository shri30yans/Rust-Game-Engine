//! Engine-owned start-up: platform context, device profile, mounted content, then game build.
use std::time::{Duration, Instant};

use crate::{AppError, ClientApp, Game, Presenter};
use engine_assets::{ContentName, ContentPack, PackReader};
use platform::{
    AppId, ConfigStore, ContentSource, DeviceProfile, Gpu, Input, PlatformApp, PresentationBinding,
};

/// A cook writes packs asynchronously; metadata checks stay well below frame frequency.
const CONTENT_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Inputs available to a game after the engine has mounted its pack and detected the device.
pub struct Start<'a> {
    /// Validated immutable game content.
    pub content: &'a ContentPack,
    /// Device class and touch capability reported by the platform.
    pub device: &'a DeviceProfile,
    /// Game-scoped settings storage.
    pub config: ConfigStore,
}

/// A statically linked game module built only after platform and content start-up.
pub trait GameEntry {
    /// Game simulation and session implementation.
    type Game: Game + 'static;
    /// Game presentation implementation.
    type Presenter: Presenter<
            Command = <Self::Game as Game>::Command,
            Settings = <Self::Game as Game>::Settings,
        >;

    /// Window title.
    const TITLE: &'static str;
    /// The game's one logical content-pack name.
    const CONTENT: ContentName;

    /// Builds the game instance after the pack and device are ready.
    fn build(start: &Start<'_>) -> Result<ClientApp<Self::Game, Self::Presenter>, AppError>;
}

pub(crate) struct PreparedGame<G: Game, P: Presenter<Command = G::Command, Settings = G::Settings>>
{
    pub(crate) client: ClientApp<G, P>,
    pub(crate) content: ContentPack,
}

/// Reads and validates the pack before invoking the game's build function.
pub(crate) fn prepare<E: GameEntry>(
    source: &ContentSource,
) -> Result<PreparedGame<E::Game, E::Presenter>, AppError> {
    let device = source.device_profile()?;
    let content = PackReader::read(source.open_content(E::CONTENT.as_str())?)?;
    let config = ConfigStore::new(AppId::new(E::CONTENT.as_str())?)?;
    let start = Start {
        content: &content,
        device: &device,
        config,
    };
    let client = E::build(&start)?;
    Ok(PreparedGame { client, content })
}

/// Starts one game through the platform-owned desktop or NativeActivity entry.
pub fn run_game<E: GameEntry>(source: ContentSource) -> Result<(), AppError> {
    source.run_with(E::TITLE, (1280, 720), || -> Result<_, AppError> {
        let PreparedGame {
            mut client,
            content,
        } = prepare::<E>(&source)?;
        client.mount_content(content)?;
        let revision = source.content_revision(E::CONTENT.as_str())?;
        Ok(ReloadingClient {
            client,
            source: &source,
            name: E::CONTENT.as_str().to_owned(),
            revision,
            last_poll: Instant::now(),
        })
    })?;
    Ok(())
}

struct ReloadingClient<'a, G: Game, P: Presenter<Command = G::Command, Settings = G::Settings>> {
    client: ClientApp<G, P>,
    source: &'a ContentSource,
    name: String,
    revision: Option<platform::ContentRevision>,
    last_poll: Instant,
}

impl<G: Game + 'static, P: Presenter<Command = G::Command, Settings = G::Settings>>
    ReloadingClient<'_, G, P>
{
    fn poll_content(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_poll) < CONTENT_POLL_INTERVAL {
            return;
        }
        self.last_poll = now;
        let revision = match self.source.content_revision(&self.name) {
            Ok(revision) => revision,
            Err(error) => {
                platform::log_error(&format!("content pack metadata: {error}"));
                return;
            }
        };
        if revision == self.revision {
            return;
        }
        let stream = match self.source.open_content(&self.name) {
            Ok(stream) => stream,
            Err(error) => {
                platform::log_error(&format!("content pack reload: {error}"));
                return;
            }
        };
        match PackReader::read(stream) {
            Ok(pack) => match self.client.mount_content(pack) {
                Ok(()) => self.revision = revision,
                Err(error) => platform::log_error(&format!("content pack reload: {error}")),
            },
            Err(error) => platform::log_error(&format!("content pack reload: {error}")),
        }
    }
}

impl<G: Game + 'static, P: Presenter<Command = G::Command, Settings = G::Settings>> PlatformApp
    for ReloadingClient<'_, G, P>
{
    fn init(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, size: (u32, u32)) {
        self.client.init(gpu, format, size);
    }

    fn resize(&mut self, size: (u32, u32)) {
        self.client.resize(size);
    }

    fn suspend(&mut self) {
        self.client.suspend();
    }

    fn resume(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, size: (u32, u32)) {
        self.client.resume(gpu, format, size);
    }

    fn on_presentation_bound(&mut self, binding: PresentationBinding) {
        self.client.on_presentation_bound(binding);
    }

    fn frame(&mut self, target: &wgpu::TextureView, input: &mut Input, dt: f32) {
        self.poll_content();
        PlatformApp::frame(&mut self.client, target, input, dt);
    }
}
