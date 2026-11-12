//! Errors returned by `engine-app`. Every failure is an explicit value; nothing panics.

use engine_assets::format::AssetId;
use engine_world::NetId;
use thiserror::Error;

/// Errors from building the app, stepping it or reading its clock.
#[derive(Debug, Error)]
pub enum AppError {
    /// The tick rate must be 1 to 1000 Hz.
    #[error("tick rate must be between 1 and 1000 Hz, got {0}")]
    InvalidTickRate(u16),
    /// A clock must be allowed at least one catch-up tick per frame.
    #[error("max catch-up ticks must be at least 1")]
    ZeroCatchUp,
    /// The world rejected a setup or simulation operation.
    #[error("world error: {0}")]
    World(#[from] engine_world::WorldError),
    /// A transport, handshake, codec, prediction or replication failure.
    #[error(transparent)]
    Net(#[from] engine_netcode::NetError),
    /// A room, lobby or resume-token failure.
    #[error(transparent)]
    Session(#[from] engine_sessions::SessionError),
    /// The server rejected this client's handshake protocol.
    #[error("server rejected the handshake: {0:?}")]
    HandshakeRejected(engine_netcode::RejectReason),
    /// A level or tuning file could not be read, parsed, validated or spawned.
    #[error("scene error: {0}")]
    Scene(#[from] engine_scene::SceneError),
    /// A UI document could not be parsed, validated or laid out.
    #[error("ui error: {0}")]
    Ui(#[from] engine_ui::UiError),
    /// A cooked asset store operation failed.
    #[error(transparent)]
    AssetStore(#[from] engine_assets::store::StoreError),
    /// A content-pack name or logical path failed validation.
    #[error(transparent)]
    ContentPath(#[from] engine_assets::InvalidContentName),
    /// A game's content pack could not be opened or validated.
    #[error(transparent)]
    ContentPack(#[from] engine_assets::PackError),
    /// A cooked GPU pack could not be prepared or uploaded.
    #[error(transparent)]
    AssetUpload(#[from] crate::asset_uploader::UploadError),
    /// A UI document file could not be read.
    #[error("ui file {path}: {source}")]
    UiFile {
        /// The file.
        path: std::path::PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// A job pool could not be built.
    #[error("job pool error: {0}")]
    Jobs(#[from] jobs::JobError),
    /// The tick counter is at `u32::MAX`; stepping again would wrap it.
    #[error("tick counter overflowed")]
    TickOverflow,
    /// The app has exhausted its monotonically increasing travel epoch.
    #[error("travel epoch overflowed")]
    TravelEpochOverflow,
    /// A local presenter requested world travel while following an online server.
    #[error("online travel is driven by the server")]
    TravelRequestedOnline,
    /// A clock or interpolation value does not fit the integer range it is stored in.
    #[error("clock remainder out of range")]
    ClockRange,
    /// A requested tick count cannot be held in a `usize` on this platform.
    #[error("tick count {0} does not fit in memory on this platform")]
    TickCountRange(u32),
    /// The frame holds fewer transforms than the world has entities with a `Transform`.
    #[error("extract needs room for {needed} transforms but the capacity is {capacity}")]
    ExtractCapacity {
        /// The largest number of transforms the frame or extractor can hold.
        capacity: usize,
        /// The number of entities with a `Transform` in the world.
        needed: usize,
    },
    /// The other end of the frame exchange was dropped.
    #[error("the other end of the frame exchange is closed")]
    ExchangeClosed,
    /// Another thread panicked while holding the frame exchange or look input lock.
    #[error("a lock shared with another thread was poisoned by a panic")]
    ExchangePoisoned,
    /// A view angle or look delta is NaN or infinite.
    #[error("view angles and look deltas must be finite")]
    NonFiniteAngle,
    /// Look sensitivity must be finite and above zero, in radians per pixel.
    #[error("look sensitivity must be finite and above zero, got {0}")]
    InvalidLookSensitivity(f32),
    /// Presenter look scale must be finite and in (0, 1].
    #[error("presenter look scale must be finite and in (0, 1], got {0}")]
    InvalidLookScale(f32),
    /// Interpolation weight must be in [0, 1].
    #[error("blend alpha must be between 0 and 1, got {0}")]
    BlendAlpha(f32),
    /// A world entity count and game budget overflow `usize` when combined.
    #[error(
        "entity capacity overflow: {entities} loaded entities plus dynamic budget {dynamic_budget}"
    )]
    EntityCapacityOverflow {
        /// Entities already present in the loaded world.
        entities: usize,
        /// Additional entities reserved for simulation dynamics.
        dynamic_budget: usize,
    },
    /// A frame time or simulation cost is NaN, infinite or negative.
    #[error("frame times must be finite and not negative, got {0} ms")]
    InvalidFrameTime(f32),
    /// A telemetry window index is out of range. The telemetry keeps its own indices in range, so
    /// this means a bug, not a state a player can reach.
    #[error("telemetry index {0} is out of range")]
    TelemetryIndex(usize),
    /// The renderer failed to build its resources or draw a frame.
    #[error("render error: {0}")]
    Render(#[from] render::RenderError),
    /// Audio playback failed.
    #[error("audio error: {0}")]
    Audio(#[from] engine_audio::AudioError),
    /// Game-provided audio data could not be parsed or validated.
    #[error("audio data error: {0}")]
    AudioConfig(String),
    /// The platform layer failed to open the window or the GPU.
    #[error("platform error: {0}")]
    Platform(#[from] platform::PlatformError),
    /// The client or presenter was asked for something it has not built yet: a frame or resize
    /// before the GPU was set up, or a camera before the presenter applied its first input.
    #[error("the client or presenter was used before it was ready")]
    ClientNotReady,
    /// The extracted frame does not hold an entity a presenter needs, such as the camera's
    /// target. The frame always holds every entity with a `Transform`, so this means the entity
    /// was never given one or was removed.
    #[error("the extracted frame has no transform for entity {0:?}")]
    MissingFrameEntity(NetId),
    /// A level instance references a cooked node that has not been registered with the presenter.
    #[error("no cooked mesh was registered for asset {0:?}")]
    MissingCookedMesh(AssetId),
    /// Cooked content referenced by a level could not be read or resolved.
    #[error("cannot load cooked level asset: {0}")]
    CookedAsset(String),
}
