//! Device texture-family selection.
//!
//! The renderer chooses one compressed family at device setup; each source texture then uses
//! that family for all of its mips, with RGBA8 as the universal fallback.

use engine_assets::GpuTextureFormatFamily;

/// Compression capabilities reported by a wgpu adapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextureCapabilities {
    /// BC compression support.
    pub bc: bool,
    /// ASTC 4x4 compression support.
    pub astc_4x4: bool,
    /// ETC2 compression support.
    pub etc2: bool,
}

/// Chooses the highest-priority supported texture family for this device.
#[must_use]
pub const fn choose_texture_family(capabilities: TextureCapabilities) -> GpuTextureFormatFamily {
    if capabilities.bc {
        GpuTextureFormatFamily::Bc7
    } else if capabilities.astc_4x4 {
        GpuTextureFormatFamily::Astc4x4
    } else if capabilities.etc2 {
        GpuTextureFormatFamily::Etc2
    } else {
        GpuTextureFormatFamily::Rgba8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_each_family_and_fallback_in_priority_order() {
        assert_eq!(
            choose_texture_family(TextureCapabilities {
                bc: true,
                astc_4x4: true,
                etc2: true
            }),
            GpuTextureFormatFamily::Bc7,
        );
        assert_eq!(
            choose_texture_family(TextureCapabilities {
                bc: false,
                astc_4x4: true,
                etc2: true
            }),
            GpuTextureFormatFamily::Astc4x4,
        );
        assert_eq!(
            choose_texture_family(TextureCapabilities {
                bc: false,
                astc_4x4: false,
                etc2: true
            }),
            GpuTextureFormatFamily::Etc2,
        );
        assert_eq!(
            choose_texture_family(TextureCapabilities::default()),
            GpuTextureFormatFamily::Rgba8,
        );
    }
}
