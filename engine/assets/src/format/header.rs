//! Fixed asset header, version, source hash, and payload integrity checks.
//!
//! The header is explicitly little-endian and payload validation is completed before data escapes.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

use super::MeshPack;
use super::texture::TextureTranscodeError;
use super::{clip, material, mesh, nodes};

const MAGIC: &[u8; 8] = b"ENGASSET";
const FORMAT_VERSION: u16 = 2;
const HEADER_BYTES: usize = 26;
const HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Errors decoding or validating cooked assets.
#[derive(Debug, Error)]
pub enum AssetError {
    /// The file header is missing or invalid.
    #[error("invalid cooked asset header")]
    Header,
    /// The file format is newer than this loader supports.
    #[error("unsupported cooked asset version {0}")]
    Version(u16),
    /// The serialized asset payload is malformed.
    #[error("invalid cooked asset payload: {0}")]
    Payload(#[from] postcard::Error),
    /// The serialized payload does not match its integrity hash.
    #[error("cooked asset payload checksum mismatch")]
    Integrity,
    /// The asset contains invalid parent/index relationships.
    #[error("invalid cooked asset structure")]
    Structure,
    /// Texture decoding or GPU format conversion failed with a typed reason.
    #[error(transparent)]
    Transcode(#[from] TextureTranscodeError),
}
/// Any typed asset payload supported by this slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Asset {
    /// Mesh pack with nodes, materials, clips and textures.
    MeshPack(MeshPack),
}
/// Encodes a cooked asset with fixed magic, version, and source hash header.
pub fn encode(asset: &Asset, source_hash: u64) -> Result<Vec<u8>, AssetError> {
    let payload = postcard::to_stdvec(asset)?;
    let mut bytes = Vec::with_capacity(HEADER_BYTES + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&source_hash.to_le_bytes());
    bytes.extend_from_slice(&hash_bytes(&payload).to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Decodes and validates a cooked asset header and payload.
pub fn decode(bytes: &[u8]) -> Result<(u64, Asset), AssetError> {
    if bytes.len() < HEADER_BYTES || bytes.get(..8) != Some(MAGIC.as_slice()) {
        return Err(AssetError::Header);
    }
    let version = u16::from_le_bytes([
        *bytes.get(8).ok_or(AssetError::Header)?,
        *bytes.get(9).ok_or(AssetError::Header)?,
    ]);
    if version != FORMAT_VERSION {
        return Err(AssetError::Version(version));
    }
    let hash_slice = bytes.get(10..18).ok_or(AssetError::Header)?;
    let source_hash_bytes: [u8; 8] = hash_slice.try_into().map_err(|_| AssetError::Header)?;
    let hash = u64::from_le_bytes(source_hash_bytes);
    let checksum_slice = bytes.get(18..26).ok_or(AssetError::Header)?;
    let checksum_bytes: [u8; 8] = checksum_slice.try_into().map_err(|_| AssetError::Header)?;
    let payload = bytes.get(HEADER_BYTES..).ok_or(AssetError::Header)?;
    if u64::from_le_bytes(checksum_bytes) != hash_bytes(payload) {
        return Err(AssetError::Integrity);
    }
    let asset: Asset = postcard::from_bytes(payload)?;
    let Asset::MeshPack(pack) = &asset;
    if pack.source_hash != hash || !valid_pack(pack) {
        return Err(AssetError::Structure);
    }
    Ok((hash, asset))
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(HASH_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(HASH_PRIME)
    })
}

fn valid_pack(pack: &MeshPack) -> bool {
    let mut collision_nodes = BTreeSet::new();
    pack.meshes
        .iter()
        .all(|record| mesh::valid_mesh(record, pack.materials.len(), pack.nodes.len()))
        && pack
            .materials
            .iter()
            .all(|record| material::is_valid(record, pack))
        && nodes::is_valid_tree(pack)
        && pack
            .clips
            .iter()
            .all(|record| clip::is_valid(record, pack.nodes.len()))
        && pack.collision.len() <= pack.nodes.len().min(mesh::MAX_COLLISION_SETS)
        && pack.collision.iter().all(|record| {
            mesh::valid_collision(record, pack.nodes.len()) && collision_nodes.insert(record.node)
        })
}
