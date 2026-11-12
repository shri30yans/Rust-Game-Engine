//! Encodes RGBA8 pixels to a deterministic Basis Universal UASTC KTX2 texture with mips.
//!
//! This is the only texture encoder in the workspace. Every cooker that writes KTX2 calls
//! [`encode_ktx2`], so the encoder label recorded in cook reports always describes the bytes
//! written. The encoder is C++ (`basisu_c_sys`), so this crate links native code.

use thiserror::Error;

/// Encoder name and version written into cook reports.
pub const ENCODER_LABEL: &str = "basisu_c_sys 0.9.1 UASTC";

/// The encoder could not produce a KTX2 texture from the given pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EncodeError {
    /// The pixels were rejected or the encoder failed to compress them.
    #[error("Basis UASTC encoding failed")]
    Basis,
}

/// Encodes one UASTC KTX2 image with deterministic colour-space and mip settings.
///
/// `rgba` holds `width * height * 4` bytes. `linear` selects linear (data) rather than sRGB
/// colour defaults, and `normal_map` uses the linear defaults that suit normal maps. The same
/// input always gives the same bytes.
///
/// # Errors
///
/// Returns [`EncodeError::Basis`] when the encoder rejects the pixels or fails to compress them.
pub fn encode_ktx2(
    rgba: &[u8],
    width: u32,
    height: u32,
    linear: bool,
    normal_map: bool,
) -> Result<Vec<u8>, EncodeError> {
    use basisu_c_sys::BasisTextureFormat;
    use basisu_c_sys::common::BU_COMP_FLAGS_GEN_MIPS_CLAMP;
    use basisu_c_sys::extra::{
        BasisuEncoder, BasisuEncoderParams, SourceImage, SourceImageFormat, basisu_encoder_init,
        types::Extent3d,
    };
    basisu_encoder_init();
    let mut encoder = BasisuEncoder::new();
    encoder
        .set_image(SourceImage {
            data: rgba,
            format: SourceImageFormat::Rgba8,
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        })
        .map_err(|_| EncodeError::Basis)?;
    // The safe 0.9.1 wrapper exposes linear/sRGB presets, but no standalone normal-map flag.
    // UASTC with linear defaults supplies normal-map metrics and filtering; it does not enable RDO.
    let mut params = if linear || normal_map {
        BasisuEncoderParams::new_with_linear_defaults(BasisTextureFormat::UastcLdr4x4)
    } else {
        BasisuEncoderParams::new_with_srgb_defaults(BasisTextureFormat::UastcLdr4x4)
    }
    .with_flags(BU_COMP_FLAGS_GEN_MIPS_CLAMP);
    params.low_level_uastc_rdo_or_dct_quality = 0.0;
    encoder.compress(params).map_err(|_| EncodeError::Basis)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Basis KTX2 files start with the KTX 2.0 identifier.
    const KTX2_IDENTIFIER: [u8; 12] = [
        0xAB, b'K', b'T', b'X', b' ', b'2', b'0', 0xBB, b'\r', b'\n', 0x1A, b'\n',
    ];

    #[test]
    fn the_same_pixels_encode_to_the_same_bytes_twice() -> Result<(), EncodeError> {
        let pixels: Vec<u8> = (0_u8..=255).cycle().take(8 * 8 * 4).collect();
        let first = encode_ktx2(&pixels, 8, 8, false, false)?;
        let second = encode_ktx2(&pixels, 8, 8, false, false)?;
        assert_eq!(first, second);
        assert!(first.starts_with(&KTX2_IDENTIFIER));
        Ok(())
    }
}
