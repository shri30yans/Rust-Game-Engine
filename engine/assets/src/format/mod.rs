//! Public facade for versioned cooked asset records.
//!
//! Record types and validation live in focused modules while this module keeps one canonical API.
mod asset_id;
mod clip;
mod header;
mod material;
mod mesh;
mod nodes;
mod texture;

pub use asset_id::{AssetId, AssetIndex, AssetLocation, InvalidAssetId};
pub use clip::{Clip, TrsChannel};
pub use header::{Asset, AssetError, decode, encode};
pub use material::Material;
pub use mesh::{
    Bounds, CollisionSet, CollisionShape, IndexEncoding, MAX_LODS, Mesh, MeshLod, MeshPack,
    PositionEncoding,
};
pub use nodes::Node;
pub use texture::{
    ColourSpace, GpuTextureFormatFamily, PreparedTexture, TextureData, TextureFamily,
    TextureTranscodeError, TranscodedMip, prepare_texture,
};

#[cfg(test)]
mod tests;
