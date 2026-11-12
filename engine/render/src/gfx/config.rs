//! Public renderer settings and per-frame submission types.

use crate::frame_plan::ViewMode;
use crate::tier::{RenderFeatures, RenderTier};
use bytemuck::{Pod, Zeroable};
use engine_core::SlotId;
use glam::{Mat4, Vec3};
use half::f16;

pub use engine_assets::ColourSpace;
/// Maximum number of cooked mesh LODs stored per renderer mesh.
pub use engine_assets::format::MAX_LODS;

/// Capacity of the per-frame instance buffer, shared by all meshes.
pub const MAX_INSTANCES: usize = 40_000;
/// Capacity of the per-frame particle buffer, split evenly between additive and alpha.
pub const MAX_PARTICLES: usize = 12_000;
/// Hard cap on point lights per frame (the GPU uniform holds this many).
pub const MAX_LIGHTS: usize = 32;

/// Quality knobs. Use [`crate::Gfx::set_settings`] to update pass-affecting values together.
/// [`crate::Gfx::set_render_scale`] is the allocation-free dynamic-resolution path.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// Quality preset; drives screen-space features and suggested defaults.
    pub tier: RenderTier,
    /// Side length of each shadow cascade map, in texels.
    pub shadow_size: u32,
    /// Requested MSAA sample count (falls back to 1 if unsupported).
    pub msaa: u32,
    /// Internal resolution scale relative to the window, 0.4 to 1.0.
    pub render_scale: f32,
    /// Percentage-closer filtering for shadow edges.
    pub pcf: bool,
    /// Point-light budget per frame, capped at `MAX_LIGHTS`.
    pub max_lights: u32,
    /// Bloom on or off.
    pub bloom: bool,
    /// Cascade split distances in metres: [near cascade end, far cascade end].
    pub splits: [f32; 2],
    /// Temporal anti-aliasing on or off. Enabling it automatically selects its required prepass.
    pub taa: bool,
    /// Presentation mode selected before the frame plan is built.
    pub view_mode: ViewMode,
}

impl Settings {
    /// Screen-space features with explicit TAA and terminal-view producers folded into the typed
    /// prepass selection.
    #[must_use]
    pub const fn features(self) -> RenderFeatures {
        self.tier
            .features()
            .with_taa(self.taa)
            .with_view_mode(self.view_mode)
    }

    /// MSAA count used at render time. A prepass has a sampleable 1x depth buffer, so it forces
    /// 1x regardless of whether TAA or a terminal debug view requested it.
    #[must_use]
    pub fn effective_msaa(self) -> u32 {
        if self.features().prepass_enabled()
            || matches!(self.view_mode, ViewMode::OpaqueCoverage { .. })
        {
            1
        } else {
            self.msaa
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            tier: RenderTier::Medium,
            shadow_size: 2048,
            msaa: 4,
            render_scale: 1.0,
            pcf: true,
            max_lights: 32,
            bloom: true,
            splits: [16.0, 70.0],
            taa: false,
            view_mode: ViewMode::Lit,
        }
    }
}

/// Compact GPU instance record. Previous transforms are stored separately for TAA.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub struct Instance {
    /// Three affine rows, each containing three basis values and translation.
    pub model_rows: [[f32; 4]; 3],
    /// Linear RGBA tint in unorm8.
    pub tint: [u8; 4],
    /// Emissive strength as f16, followed by two bytes of padding.
    pub emissive: [u16; 2],
    /// Pattern, roughness, metalness, flags, and reserved packed bytes.
    pub material: [u8; 8],
}

/// Previous affine transform bound as a separate 48-byte stream when TAA is enabled.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub struct PreviousTransform {
    /// Three affine rows, each containing three basis values and translation.
    pub rows: [[f32; 4]; 3],
}

impl PreviousTransform {
    /// Packs a previous transform into three affine rows.
    #[must_use]
    pub fn from_mat4(matrix: Mat4) -> Self {
        let columns = matrix.to_cols_array_2d();
        Self {
            rows: [
                [columns[0][0], columns[1][0], columns[2][0], columns[3][0]],
                [columns[0][1], columns[1][1], columns[2][1], columns[3][1]],
                [columns[0][2], columns[1][2], columns[2][2], columns[3][2]],
            ],
        }
    }
}

/// Instance flag: the pattern is evaluated in object space (sticks to the mesh).
pub const FLAG_OBJECT_SPACE: u32 = 1;
/// Instance flag: skip lighting and shading.
pub const FLAG_UNLIT: u32 = 2;

impl Instance {
    /// Instance at `m` with white tint, no emission and the vertex material.
    pub fn new(m: Mat4) -> Instance {
        let c = m.to_cols_array_2d();
        Instance {
            model_rows: [
                [c[0][0], c[1][0], c[2][0], c[3][0]],
                [c[0][1], c[1][1], c[2][1], c[3][1]],
                [c[0][2], c[1][2], c[2][2], c[3][2]],
            ],
            tint: [u8::MAX; 4],
            emissive: [0; 2],
            material: [u8::MAX, u8::MAX, u8::MAX, 0, 0, 0, 0, 0],
        }
    }
    /// Sets the linear RGB tint.
    pub fn tint(mut self, c: [f32; 3]) -> Self {
        self.tint[0] = crate::mesh::pack_unorm8(c[0]);
        self.tint[1] = crate::mesh::pack_unorm8(c[1]);
        self.tint[2] = crate::mesh::pack_unorm8(c[2]);
        self
    }
    /// Sets the emissive strength.
    pub fn emissive(mut self, e: f32) -> Self {
        self.emissive[0] = f16::from_f32(e).to_bits();
        self
    }
    /// Overrides the procedural pattern.
    pub fn pattern(mut self, p: crate::Pattern) -> Self {
        self.material[0] = p as u8;
        self
    }
    /// Overrides roughness (0 to 1).
    pub fn rough(mut self, r: f32) -> Self {
        self.material[1] = crate::mesh::pack_unorm8(r);
        self
    }
    /// Overrides metalness (0 to 1).
    pub fn metal(mut self, m: f32) -> Self {
        self.material[2] = crate::mesh::pack_unorm8(m);
        self
    }
    /// Sets the flag bits (`FLAG_*`).
    pub fn flags(mut self, f: u32) -> Self {
        self.material[3] = u8::try_from(f).unwrap_or(u8::MAX);
        self
    }

    pub(crate) fn mesh_index(mut self, index: u32) -> Self {
        let bytes = index.to_le_bytes();
        let [pattern, roughness, metalness, flags, _, _, _, _] = self.material;
        let [x, y, z, w] = bytes;
        self.material = [pattern, roughness, metalness, flags, x, y, z, w];
        self
    }
}

/// Billboard particle. `kind`: 0 glow, 1 spark, 2 smoke, 3 ring, 4 disc.
/// A non-zero `vel` stretches the sprite along it (world units).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Particle {
    /// World-space centre.
    pub pos: [f32; 3],
    /// Sprite radius in world units.
    pub size: f32,
    /// Linear RGBA colour.
    pub color: [f32; 4],
    /// Velocity, used to stretch the sprite.
    pub vel: [f32; 3],
    /// Sprite kind (see the type docs).
    pub kind: f32,
}

/// Viewer camera. Angles are radians; `fov_y` is the world projection, `vm_fov_y` the view model.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// World-space eye position.
    pub pos: Vec3,
    /// Heading, radians (see [`forward`]).
    pub yaw: f32,
    /// Pitch, radians; positive looks up.
    pub pitch: f32,
    /// Roll about the view axis, radians.
    pub roll: f32,
    /// Vertical field of view of the world, radians.
    pub fov_y: f32,
    /// Vertical field of view of the view model, radians.
    pub vm_fov_y: f32,
    /// Near clip distance, metres.
    pub near: f32,
    /// Far clip distance, metres.
    pub far: f32,
}

impl Camera {
    /// Unit view direction.
    pub fn forward(&self) -> Vec3 {
        forward(self.yaw, self.pitch)
    }
    /// World-to-view matrix.
    pub fn view(&self) -> Mat4 {
        let f = self.forward();
        let right = f.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(f);
        let up = glam::Quat::from_axis_angle(f, self.roll) * up;
        Mat4::look_to_rh(self.pos, f, up)
    }
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            pos: Vec3::new(0.0, 1.7, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov_y: 1.2,
            vm_fov_y: 0.95,
            near: 0.05,
            far: 600.0,
        }
    }
}

/// Yaw 0 looks down -Z; positive yaw turns left (counter-clockwise from above).
pub fn forward(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(
        -yaw.sin() * pitch.cos(),
        pitch.sin(),
        -yaw.cos() * pitch.cos(),
    )
}

/// Lighting and atmosphere. Colours are linear.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    /// Direction towards the sun.
    pub sun_dir: Vec3,
    /// Sun colour, linear RGB.
    pub sun_color: [f32; 3],
    /// Sun brightness multiplier.
    pub sun_intensity: f32,
    /// Sky colour straight up, linear RGB.
    pub sky_zenith: [f32; 3],
    /// Sky colour at the horizon, linear RGB.
    pub sky_horizon: [f32; 3],
    /// Colour of the ground plane seen below the horizon, linear RGB.
    pub ground: [f32; 3],
    /// Ambient light multiplier.
    pub ambient: f32,
    /// Fog density.
    pub fog_density: f32,
    /// How quickly fog thickens with height.
    pub fog_falloff: f32,
    /// Fog base height.
    pub fog_base: f32,
    /// Strength of in-scattered sun light in fog.
    pub fog_inscatter: f32,
    /// Fog colour, linear RGB.
    pub fog_color: [f32; 3],
    /// Cloud coverage amount.
    pub clouds: f32,
}

impl Default for Env {
    fn default() -> Self {
        Env {
            sun_dir: Vec3::new(0.45, 0.62, 0.35).normalize(),
            sun_color: [1.0, 0.9, 0.78],
            sun_intensity: 3.6,
            sky_zenith: [0.16, 0.32, 0.62],
            sky_horizon: [0.62, 0.7, 0.8],
            ground: [0.22, 0.2, 0.18],
            ambient: 1.0,
            fog_density: 0.006,
            fog_falloff: 0.06,
            fog_base: 0.0,
            fog_inscatter: 0.6,
            fog_color: [0.55, 0.62, 0.72],
            clouds: 0.5,
        }
    }
}

/// Post-processing controls (game code animates damage/flash per frame).
#[derive(Clone, Copy, Debug)]
pub struct Post {
    /// Exposure multiplier before tonemapping.
    pub exposure: f32,
    /// Bloom strength.
    pub bloom: f32,
    /// Luminance above which bloom starts.
    pub bloom_threshold: f32,
    /// Vignette strength.
    pub vignette: f32,
    /// Film grain strength.
    pub grain: f32,
    /// Damage overlay strength, 0 to 1.
    pub damage: f32,
    /// Full-screen flash strength, 0 to 1.
    pub flash: f32,
    /// Flash colour, linear RGB.
    pub flash_color: [f32; 3],
    /// Saturation multiplier (1 is neutral).
    pub saturation: f32,
    /// Contrast multiplier (1 is neutral).
    pub contrast: f32,
    /// Blend toward greyscale, 0 to 1.
    pub desaturate: f32,
    /// Colour added to shadows (RGB) with strength in alpha.
    pub shadow_tint: [f32; 4],
    /// Colour added to highlights (RGB) with strength in alpha.
    pub highlight_tint: [f32; 4],
}

impl Default for Post {
    fn default() -> Self {
        Post {
            exposure: 1.0,
            bloom: 0.06,
            bloom_threshold: 1.2,
            vignette: 0.28,
            grain: 0.025,
            damage: 0.0,
            flash: 0.0,
            flash_color: [1.0, 1.0, 1.0],
            saturation: 1.08,
            contrast: 1.04,
            desaturate: 0.0,
            shadow_tint: [0.05, 0.12, 0.2, 0.05],
            highlight_tint: [0.2, 0.12, 0.02, 0.04],
        }
    }
}

/// Vertex stream format selected when a mesh is uploaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeshFormat {
    /// Position plus procedural normal, colour and pattern attributes.
    Procedural,
    /// Cooked 32-bit positions and cooked normal, tangent and UV attributes.
    CookedFloat32,
    /// Cooked 16-bit quantized positions and cooked normal, tangent and UV attributes.
    CookedQuantized16,
}

impl MeshFormat {
    pub(crate) const fn pipeline_index(self) -> usize {
        match self {
            Self::Procedural => 0,
            Self::CookedFloat32 => 1,
            Self::CookedQuantized16 => 2,
        }
    }
}

/// Handle to a mesh uploaded with [`Gfx::add_mesh`] or [`Gfx::add_cooked_mesh`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct MeshId(pub(crate) SlotId, pub(crate) MeshFormat);

impl MeshId {
    pub(crate) const fn new(slot: SlotId, format: MeshFormat) -> Self {
        Self(slot, format)
    }

    /// Creates a procedural mesh id for fixed engine-owned fallback records.
    #[must_use]
    pub const fn procedural(index: u32) -> Self {
        Self(SlotId::new(index, 1), MeshFormat::Procedural)
    }

    /// Slot index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0.index()
    }

    /// Slot generation.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.0.generation()
    }

    /// Vertex stream format.
    #[must_use]
    pub const fn format(self) -> MeshFormat {
        self.1
    }
}

/// Generation-checked texture handle owned by [`super::Gfx`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub(crate) SlotId);

impl TextureId {
    /// Slot index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0.index()
    }
    /// Slot generation.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.0.generation()
    }
}

/// Generation-checked handle to a renderer-owned PBR material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialId(pub(crate) SlotId);

impl MaterialId {
    /// Slot index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0.index()
    }

    /// Slot generation.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.0.generation()
    }
}

/// Alpha handling for an opaque material or an alpha-tested cutout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlphaMode {
    /// Render every texel as opaque.
    Opaque,
    /// Discard texels whose final alpha is below the cutoff.
    Mask(f32),
}

/// Texture set and physically based factors for one shared material bind group.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialDesc {
    /// Linear base colour multiplier.
    pub base_color: [f32; 4],
    /// Metallic factor.
    pub metallic: f32,
    /// Roughness factor.
    pub roughness: f32,
    /// Emissive intensity multiplier.
    pub emissive_strength: f32,
    /// Tangent-space normal intensity multiplier.
    pub normal_scale: f32,
    /// Opaque rendering or alpha-tested masking.
    pub alpha_mode: AlphaMode,
    /// Optional sRGB base colour texture.
    pub base_color_texture: Option<TextureId>,
    /// Optional linear tangent-space normal texture.
    pub normal_texture: Option<TextureId>,
    /// Optional linear occlusion-roughness-metal texture.
    pub orm_texture: Option<TextureId>,
    /// Optional sRGB emissive texture.
    pub emissive_texture: Option<TextureId>,
}

impl Default for MaterialDesc {
    fn default() -> Self {
        Self {
            base_color: [1.0; 4],
            metallic: 0.0,
            roughness: 1.0,
            emissive_strength: 0.0,
            normal_scale: 1.0,
            alpha_mode: AlphaMode::Opaque,
            base_color_texture: None,
            normal_texture: None,
            orm_texture: None,
            emissive_texture: None,
        }
    }
}

/// Counters for the most recent frame.
#[derive(Default, Clone, Copy, Debug)]
pub struct FrameStats {
    /// Draw calls issued in the scene and composite passes.
    pub draws: u32,
    /// Draw calls issued in the shadow passes.
    pub shadow_draws: u32,
    /// Triangles submitted (instances times triangles per mesh).
    pub tris: u64,
    /// Mesh instances drawn.
    pub instances: u32,
    /// Particles drawn.
    pub particles: u32,
    /// Visible primitive counts by selected mesh LOD, coarse LODs at higher indexes.
    pub lod_counts: [u32; MAX_LODS],
    /// Instances rejected by frustum culling.
    pub culled: u32,
    /// Bytes uploaded for retained instance records during this frame.
    pub instance_bytes_uploaded: u64,
    /// Retained primitive slots changed before this frame was drawn.
    pub dirty_slots: u32,
    /// Camera-visible retained primitives submitted to the main pass.
    pub visible_main: u32,
    /// Shadow-visible retained primitives submitted to each cascade.
    pub visible_shadow: [u32; 2],
    /// CPU nanoseconds spent culling retained slots.
    pub cull_time_ns: u64,
    /// CPU nanoseconds spent flattening and uploading retained slots and indices.
    pub upload_time_ns: u64,
    /// CPU nanoseconds spent encoding render passes and submitting the command buffer.
    pub encode_time_ns: u64,
    /// Resident GPU texture bytes by BC7, ASTC 4x4, ETC2 and RGBA8 family.
    pub texture_bytes_by_family: [u64; 4],
    /// Cooked mesh and texture bytes uploaded during this frame.
    pub asset_upload_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_taa(tier: RenderTier, taa: bool, msaa: u32) -> Settings {
        Settings {
            tier,
            taa,
            msaa,
            ..Settings::default()
        }
    }

    #[test]
    fn taa_request_always_creates_a_valid_prepass_feature() {
        for tier in [RenderTier::Medium, RenderTier::High, RenderTier::Ultra] {
            let settings = with_taa(tier, true, 4);
            assert!(settings.features().prepass_enabled(), "{tier:?}");
            assert!(settings.features().taa(), "{tier:?}");
            assert!(crate::FramePlan::build(settings).is_ok(), "{tier:?}");
        }
    }

    #[test]
    fn taa_off_keeps_the_tier_prepass_requirement() {
        assert!(
            !with_taa(RenderTier::Medium, false, 4)
                .features()
                .prepass_enabled()
        );
        assert!(
            with_taa(RenderTier::High, false, 4)
                .features()
                .prepass_enabled()
        );
    }

    #[test]
    fn taa_forces_one_sample_msaa() {
        assert_eq!(with_taa(RenderTier::High, true, 4).effective_msaa(), 1);
        assert_eq!(with_taa(RenderTier::High, false, 4).effective_msaa(), 1);
        assert_eq!(with_taa(RenderTier::Medium, false, 4).effective_msaa(), 4);
    }

    #[test]
    fn features_mirror_the_taa_setting() {
        assert!(with_taa(RenderTier::High, true, 1).features().taa());
        assert!(!with_taa(RenderTier::High, false, 1).features().taa());
    }

    #[test]
    fn instance_record_is_exactly_64_bytes() {
        assert_eq!(std::mem::size_of::<Instance>(), 64);
    }

    #[test]
    fn affine_rows_preserve_translation_and_basis_values() {
        let matrix = Mat4::from_scale_rotation_translation(
            glam::Vec3::new(2.0, 3.0, 0.5),
            glam::Quat::from_rotation_y(0.37),
            glam::Vec3::new(4.0, -2.0, 9.0),
        );
        let packed = Instance::new(matrix);
        let rows = packed.model_rows;
        let columns = matrix.to_cols_array_2d();
        let expected = [
            [columns[0][0], columns[1][0], columns[2][0], columns[3][0]],
            [columns[0][1], columns[1][1], columns[2][1], columns[3][1]],
            [columns[0][2], columns[1][2], columns[2][2], columns[3][2]],
        ];
        for (actual_row, expected_row) in rows.iter().zip(expected) {
            for (actual, expected) in actual_row.iter().zip(expected_row) {
                assert!((actual - expected).abs() <= f32::EPSILON);
            }
        }
    }

    #[test]
    fn tint_and_material_overrides_pack_to_exact_bytes() {
        let packed = Instance::new(Mat4::IDENTITY)
            .tint([0.0, 128.0 / 255.0, 1.0])
            .rough(0.5)
            .metal(1.0);
        assert_eq!(&packed.tint[..3], &[0, 128, 255]);
        assert_eq!(packed.material[1], 128);
        assert_eq!(packed.material[2], 255);
    }
}
