//! Forward+ style HDR renderer for the arena games: instanced meshes with
//! procedural world-space materials, 2-cascade sun shadows, point lights,
//! particles, bloom, ACES tonemap and an SDF UI layer with TTF text.
//!
//! Development shader hot reload: `cargo run -p engine-render --example smoke --features hot-reload`.
#![forbid(unsafe_code)]

pub(crate) const BLOOM_LEVELS: usize = 5;

mod error;
mod font;
mod frame_plan;
mod gfx;
/// Test-only PNG golden comparison and plausibility checks.
#[cfg(feature = "golden")]
pub mod golden;
mod governor;
#[cfg(feature = "hot-reload")]
mod hot_reload;
mod lod;
mod mesh;
mod scene;
mod screen_space;
mod shaders;
mod taa;
mod targets;
mod texture;
mod tier;
mod tiles;
mod timer;
mod ui;

pub use error::RenderError;
pub use font::{ATLAS, FontAtlas, FontId};
pub use frame_plan::{
    FrameInput, FramePass, FramePlan, FrameTarget, OpaqueDepth, PostSource, ViewMode,
};
pub use gfx::*;
pub use governor::{Governor, GovernorConfig};
pub use lod::{LOD_HYSTERESIS, select_lod, shadow_lod};
pub use mesh::{Mat, MeshBuilder, Pattern, Vertex, VertexAttributes};
pub use scene::{CullStats, Layer, Look, Material, PrimitiveDesc, PrimitiveId, RenderScene};
pub use shaders::ShaderId;
pub use taa::{JITTER_SAMPLES, Jitter, halton, jittered_projection};
pub use texture::{TextureCapabilities, choose_texture_family};
pub use tier::{
    DESKTOP_UPLOAD_BUDGET_BYTES, Gtao, PHONE_UPLOAD_BUDGET_BYTES, Prepass, RenderFeatures,
    RenderTier,
};
pub use timer::{GPU_PASSES, GpuPass, GpuTimes};
pub use ui::{Align, MAX_UI_VERTS, Tilt, Ui, UiUniform, UiVertex, rgba};

/// sRGB hex (0xRRGGBB) to linear RGB.
pub fn srgb(hex: u32) -> [f32; 3] {
    let c = |s: u32| {
        let value = ((hex >> s) & 255) as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    [c(16), c(8), c(0)]
}

/// Converts a count or index to `u32`, saturating at `u32::MAX`. Only reachable for
/// buffers far larger than the GPU limits used here.
pub(crate) fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Scales a pixel dimension upward so every partially covered pixel has storage.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn scale_px_ceil(v: u32, scale: f32) -> u32 {
    (v as f32 * scale).ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::srgb;

    #[test]
    fn srgb_uses_the_piecewise_transfer_curve() {
        assert_eq!(srgb(0x000000), [0.0, 0.0, 0.0]);
        let gray = srgb(0x808080);
        assert!((gray[0] - 0.21586).abs() < 0.00002);
        let white = srgb(0xffffff);
        assert_eq!(white, [1.0, 1.0, 1.0]);
        let threshold = srgb(0x0a0a0a);
        assert!((threshold[0] - (10.0 / 255.0 / 12.92)).abs() < 1e-7);
    }
}
