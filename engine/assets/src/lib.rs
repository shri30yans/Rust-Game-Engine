//! Game-agnostic cooked asset formats and loading.
//!
//! Cooked data is validated before it enters the store; frame reads are immutable and do not
//! acquire locks. Filesystem access and texture transcoding stay on background workers.
#![forbid(unsafe_code)]

pub mod format;
pub mod handle;
pub mod load;
pub mod pack;
pub mod path;
pub mod store;

pub use format::{
    Asset, AssetError, ColourSpace, GpuTextureFormatFamily, MeshPack, PreparedTexture, TextureData,
    TextureFamily, TextureTranscodeError, prepare_texture,
};
pub use handle::Handle;
pub use pack::{ContentPack, PackEntry, PackError, PackReader};
pub use path::{AssetPath, ContentName, InvalidContentName};
pub use store::{AssetFailure, AssetKind, AssetStatus, AssetStore, Assets, StoreEvent};

#[cfg(test)]
mod pack_tests;
