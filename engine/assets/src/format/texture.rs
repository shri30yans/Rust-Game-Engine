//! Basis KTX2 texture records and GPU-family transcoding.
//!
//! The renderer chooses a family; this module returns each transcoded mip without GPU dependencies.
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::header::AssetError;

/// Typed failure category returned by Basis KTX2 decoding and transcoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TextureTranscodeError {
    /// The input is not a valid Basis KTX2 texture.
    #[error("invalid KTX2 Basis texture")]
    InvalidKtx2,
    /// The requested target format is unavailable for the encoded source.
    #[error("texture cannot be transcoded to the requested GPU format")]
    UnsupportedTarget,
    /// The encoded image level failed during transcode.
    #[error("Basis image level could not be transcoded")]
    InvalidLevel,
}

/// GPU texture format families supported by the engine's device profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuTextureFormatFamily {
    /// Desktop BPTC compressed format.
    Bc7,
    /// Mobile ASTC 4x4 compressed format.
    Astc4x4,
    /// ETC2 compressed format.
    Etc2,
    /// Uncompressed RGBA8 fallback.
    Rgba8,
}

/// Device texture compression family selected before transcoding.
pub type TextureFamily = GpuTextureFormatFamily;

/// Colour interpretation for texture sampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourSpace {
    /// Colour values decoded from sRGB to linear for shading.
    Srgb,
    /// Linear data such as normal maps and ORM.
    Linear,
}

/// Decoded texture mip and dimensions ready for renderer upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodedMip {
    /// Pixel width of this mip.
    pub width: u32,
    /// Pixel height of this mip.
    pub height: u32,
    /// Encoded GPU data for this mip.
    pub bytes: Vec<u8>,
}

/// CPU texture data prepared for upload to one texture family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTexture {
    /// Encoded data for every mip level.
    pub mips: Vec<TranscodedMip>,
    /// Target GPU family for these mip bytes.
    pub family: TextureFamily,
    /// Sampling interpretation used to select the GPU format.
    pub colour_space: ColourSpace,
}

/// KTX2 Basis texture retained in its compact source representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureData {
    /// Complete KTX2 file bytes.
    pub ktx2: Vec<u8>,
}

impl TextureData {
    /// Transcodes every mip for the caller-selected GPU family.
    pub fn transcode(
        &self,
        target: GpuTextureFormatFamily,
    ) -> Result<Vec<TranscodedMip>, AssetError> {
        let transcoder = basisu::Transcoder::new(&self.ktx2)
            .map_err(|_| AssetError::from(TextureTranscodeError::InvalidKtx2))?;
        let format = match target {
            GpuTextureFormatFamily::Bc7 => basisu::TargetFormat::Bc7Rgba,
            GpuTextureFormatFamily::Astc4x4 => basisu::TargetFormat::Astc4x4Rgba,
            GpuTextureFormatFamily::Etc2 => basisu::TargetFormat::Etc2Rgba,
            GpuTextureFormatFamily::Rgba8 => basisu::TargetFormat::Rgba32,
        };
        let (width, height) = transcoder.base_dimensions();
        let mut result = Vec::new();
        for level in 0..transcoder.level_count() {
            let bytes = transcoder
                .transcode(level, format, basisu::DecodeFlags::NONE)
                .map_err(|_| AssetError::from(TextureTranscodeError::InvalidLevel))?;
            result.push(TranscodedMip {
                width: width
                    .checked_shr(level)
                    .map_or(1, |dimension| dimension.max(1)),
                height: height
                    .checked_shr(level)
                    .map_or(1, |dimension| dimension.max(1)),
                bytes,
            });
        }
        Ok(result)
    }
}

/// Transcodes a texture into every mip required by the selected device family.
pub fn prepare_texture(
    source: &TextureData,
    colour_space: ColourSpace,
    family: TextureFamily,
) -> Result<PreparedTexture, AssetError> {
    Ok(PreparedTexture {
        mips: source.transcode(family)?,
        family,
        colour_space,
    })
}
