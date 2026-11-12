//! Typed errors for renderer resource setup.
use crate::gfx::TextureId;
use crate::gfx::{MaterialId, MeshId};
use crate::scene::PrimitiveId;
use crate::shaders::ShaderId;
use std::path::PathBuf;
use thiserror::Error;

/// Errors from building renderer resources.
#[derive(Debug, Error)]
pub enum RenderError {
    /// The font data could not be parsed; the payload is the parser's reason.
    #[error("font could not be parsed: {0}")]
    BadFont(&'static str),
    /// The atlas already holds 256 fonts, the most a `FontId` (u8) can index.
    #[error("too many fonts: the atlas holds at most 256")]
    TooManyFonts,
    /// WGSL validation or parse failed for this shader module.
    #[error("shader `{id}` failed to compile: {message}")]
    ShaderCompile {
        /// Which module was being built.
        id: ShaderId,
        /// Message from the GPU driver / wgpu validation layer.
        message: String,
    },
    /// Shader source file could not be read (hot-reload builds only).
    #[error("could not read shader file {path}")]
    ShaderRead {
        /// Path that was opened.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// The checked frame plan found a pass input without an earlier source.
    #[error("invalid frame plan: {pass:?} reads unavailable target {target:?}")]
    InvalidFramePlan {
        /// The pass whose declared read could not be satisfied.
        pass: crate::FramePass,
        /// The target absent from all earlier passes and external inputs.
        target: crate::FrameTarget,
    },
    /// The retained primitive handle is stale or has been removed.
    #[error("stale render primitive {0:?}")]
    StalePrimitive(PrimitiveId),
    /// The scene slot index space is exhausted.
    #[error("render scene has exhausted its primitive slot space")]
    SceneCapacity,
    /// A primitive refers to a mesh that has not been uploaded.
    #[error("unknown render mesh {0:?}")]
    UnknownMesh(MeshId),
    /// A mesh cannot be removed while a retained primitive references it.
    #[error("render mesh is still used by a live primitive {0:?}")]
    MeshInUse(MeshId),
    /// A retained primitive still uses a material being removed.
    #[error("render material is still used by a live primitive {0:?}")]
    MaterialInUse(MaterialId),
    /// Material handle is stale or does not identify an uploaded material.
    #[error("unknown render material {0:?}")]
    UnknownMaterial(MaterialId),
    /// Material factors or alpha cutoff are outside the supported finite range.
    #[error("material factors must be finite and normalized")]
    InvalidMaterialDesc,
    /// Cooked mesh stream lengths or bounds do not match the declared format and counts.
    #[error("cooked mesh streams do not match their declared format and counts")]
    InvalidCookedMesh,
    /// Transcoded asset data could not be uploaded as a GPU texture.
    #[error("texture transcoding or upload failed: {0}")]
    TextureUpload(#[from] engine_assets::format::AssetError),
    /// Prepared texture bytes target a different format family from this device.
    #[error("prepared texture family does not match the renderer device")]
    TextureFamilyMismatch,
    /// Texture handle is stale or does not identify an uploaded texture.
    #[error("unknown render texture {0:?}")]
    UnknownTexture(TextureId),
    /// Texture cannot be removed while an uploaded material references it.
    #[error("render texture is used by a live material {0:?}")]
    TextureInUse(TextureId),
    /// A texture has no remaining representable material reference slots.
    #[error("texture material reference count is exhausted {0:?}")]
    TextureUseCountOverflow(TextureId),
    /// The frame governor was built with values outside its range; the payload says which rule
    /// failed.
    #[error("invalid frame governor: {0}")]
    InvalidGovernor(&'static str),
    /// The frame governor was given a frame time that is NaN, infinite or negative.
    #[error("frame time must be finite and not negative, got {0} ms")]
    InvalidFrameTime(f32),
    /// Shader watcher could not read file metadata.
    #[error("shader watch failed on {path}")]
    ShaderWatch {
        /// File whose metadata was requested.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
}
