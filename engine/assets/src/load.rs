//! File decoding for cooked assets.
//!
//! Loads validate the complete versioned payload before exposing data to the store.
use thiserror::Error;

use crate::format::{self, Asset, AssetError, MeshPack};

/// Errors reading or decoding a cooked asset file.
#[derive(Debug, Error)]
pub enum LoadError {
    /// File system operation failed.
    #[error("asset I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Cooked file is invalid.
    #[error(transparent)]
    Format(#[from] AssetError),
    /// The cooked asset does not contain a mesh pack.
    #[error("cooked asset is not a mesh pack")]
    WrongType,
    /// Entry bytes came from an invalid or missing pack entry.
    #[error(transparent)]
    Pack(#[from] crate::PackError),
}

/// Decodes a mesh pack entry from validated content bytes.
pub fn load_mesh_pack(bytes: &[u8]) -> Result<MeshPack, LoadError> {
    let (_, asset) = format::decode(bytes)?;
    match asset {
        Asset::MeshPack(pack) => Ok(pack),
    }
}

/// Validates a cooked KTX2 Basis texture entry.
pub fn load_texture(ktx2: &[u8]) -> Result<crate::format::TextureData, LoadError> {
    basisu::Transcoder::new(ktx2)
        .map_err(|_| AssetError::from(crate::format::TextureTranscodeError::InvalidKtx2))?;
    Ok(crate::format::TextureData {
        ktx2: ktx2.to_vec(),
    })
}
