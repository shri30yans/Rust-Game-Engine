//! The renderer. Owns retained scene slots, their GPU mirrors, and every pipeline.
//!
//! Frame: 2 shadow cascades -> optional prepass (normals, depth, motion) + GTAO -> HDR scene
//! -> optional TAA resolve -> bloom -> optional auto exposure -> composite (tonemap, grade)
//! and UI to the swapchain. With TAA the projection is jittered by a 16-sample Halton sequence,
//! and history is kept unjittered for motion and sky reprojection.
mod config;
mod frame;
pub(crate) mod pipelines;
#[cfg(any(feature = "hot-reload", test))]
mod reload;
pub(crate) mod setup;

pub use config::*;

use crate::gfx::pipelines::PipelineLayouts;
use crate::mesh::MeshBuilder;
use crate::scene::{Material, RenderScene};
use crate::screen_space;
use crate::screen_space::{FullscreenOptions, ScreenSpace};
use crate::taa::{Jitter, jittered_projection};
use crate::targets::{self, FrameTargetDesc, FrameTargets, TaaTargetDesc};
use crate::tiles::Tiles;
use crate::timer::{GpuPass, GpuTimer, GpuTimes};
use crate::ui::{MAX_UI_VERTS, Ui, UiVertex};
use crate::{RenderError, scale_px_ceil, to_u32};
use bytemuck::{Pod, Zeroable};
use engine_assets::format::{Bounds, IndexEncoding, MeshLod, PositionEncoding};
use engine_assets::{GpuTextureFormatFamily, PreparedTexture};
use engine_core::Slots;
use glam::{Mat4, Vec3, Vec4};
use platform::Gpu;

pub(crate) const INITIAL_SCENE_CAPACITY: usize = 256;
const INDEXES_PER_PRIMITIVE: usize = 3;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct Light {
    pos_r: [f32; 4],
    col_i: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct Globals {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    vm_proj: [[f32; 4]; 4],
    cam_world: [[f32; 4]; 4],
    cascade: [[[f32; 4]; 4]; 2],
    cam_pos: [f32; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    sky_zenith: [f32; 4],
    sky_horizon: [f32; 4],
    ground: [f32; 4],
    fog: [f32; 4],
    fog_color: [f32; 4],
    shadow: [f32; 4],
    misc: [f32; 4],
    tiles: [u32; 4],
    lights: [Light; MAX_LIGHTS],
    /// View-projection without the TAA jitter (motion vectors and sky reprojection).
    unjit_view_proj: [[f32; 4]; 4],
    /// Last frame's unjittered view-projection.
    prev_view_proj: [[f32; 4]; 4],
    /// x = 1 when the TAA history holds a previous frame, else 0.
    taa: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PostU {
    a: [f32; 4],
    b: [f32; 4],
    c: [f32; 4],
    shadows: [f32; 4],
    highlights: [f32; 4],
    flash_color: [f32; 4],
    d: [f32; 4],
    e: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
    factors: [f32; 4],
    alpha: [f32; 4],
}

type Targets = FrameTargets;

struct LodRange {
    vb: wgpu::Buffer,
    attribute_vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    indices: std::ops::Range<u32>,
    vertex_base: i32,
    index_format: wgpu::IndexFormat,
    screen_size_threshold: f32,
    format: MeshFormat,
}

struct GpuMesh {
    lods: [Option<LodRange>; MAX_LODS],
    lod_count: usize,
    bytes: u64,
}

impl GpuMesh {
    fn lod(&self, index: usize) -> Option<&LodRange> {
        self.lods.get(index).and_then(Option::as_ref)
    }

    fn thresholds(&self) -> [f32; MAX_LODS] {
        std::array::from_fn(|index| self.lod(index).map_or(0.0, |lod| lod.screen_size_threshold))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
struct MeshRecord {
    minimum: [f32; 4],
    maximum: [f32; 4],
    format: [u32; 4],
}

impl MeshRecord {
    fn new(bounds: Bounds, format: MeshFormat) -> Self {
        let format = match format {
            MeshFormat::Procedural => 0,
            MeshFormat::CookedFloat32 => 1,
            MeshFormat::CookedQuantized16 => 2,
        };
        Self {
            minimum: [bounds.min[0], bounds.min[1], bounds.min[2], 0.0],
            maximum: [bounds.max[0], bounds.max[1], bounds.max[2], 0.0],
            format: [format, 0, 0, 0],
        }
    }
}

struct GpuTexture {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    family: GpuTextureFormatFamily,
    bytes: u64,
    material_uses: u32,
}

struct GpuMaterial {
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    textures: [Option<TextureId>; 4],
}

struct GpuDefaultTexture {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct IndexBucket {
    mesh: MeshId,
    lod: usize,
    material: Option<MaterialId>,
    shadow: [Vec<u32>; 2],
    both: Vec<u32>,
    main_only: Vec<u32>,
    vm: Vec<u32>,
    glow: Vec<u32>,
    ranges: Ranges,
}

impl IndexBucket {
    fn new(mesh: MeshId, lod: usize, material: Option<MaterialId>) -> Self {
        Self {
            mesh,
            lod,
            material,
            shadow: std::array::from_fn(|_| Vec::with_capacity(256)),
            both: Vec::with_capacity(256),
            main_only: Vec::with_capacity(256),
            vm: Vec::with_capacity(64),
            glow: Vec::with_capacity(64),
            ranges: Ranges::default(),
        }
    }

    fn reserve_slots(&mut self, capacity: usize) {
        for indices in &mut self.shadow {
            indices.reserve(capacity.saturating_sub(indices.len()));
        }
        self.both.reserve(capacity.saturating_sub(self.both.len()));
        self.main_only
            .reserve(capacity.saturating_sub(self.main_only.len()));
        self.vm.reserve(capacity.saturating_sub(self.vm.len()));
        self.glow.reserve(capacity.saturating_sub(self.glow.len()));
    }
}

#[derive(Clone, Copy, Default)]
struct Ranges {
    start: u32,
    shadow_start: [u32; 2],
    shadow_count: [u32; 2],
    both: u32,
    mo: u32,
    vm_start: u32,
    vm: u32,
    glow_start: u32,
    glow: u32,
}

/// Prebuilt source bindings for terminal renderer buffer visualisation.
struct DebugBindings {
    hdr: wgpu::BindGroup,
    ambient_occlusion: Option<wgpu::BindGroup>,
    depth: Option<wgpu::BindGroup>,
    coverage: Option<wgpu::BindGroup>,
    shadow: wgpu::BindGroup,
}

/// The renderer. Owns the device, all pipelines and per-frame buffers.
pub struct Gfx {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    /// Window size in pixels.
    pub size: (u32, u32),
    /// Current quality settings. All updates must use [`Gfx::set_settings`].
    settings: Settings,
    /// Immutable checked pass order for the current settings.
    plan: crate::FramePlan,
    msaa: u32,
    taa_history_format: wgpu::TextureFormat,
    meshes: Slots<GpuMesh>,
    mesh_records: Vec<MeshRecord>,
    mesh_records_buf: wgpu::Buffer,
    mesh_record_capacity: usize,
    textures: Slots<GpuTexture>,
    materials: Slots<GpuMaterial>,
    texture_family: GpuTextureFormatFamily,
    material_sampler: wgpu::Sampler,
    default_material_textures: [GpuDefaultTexture; 4],
    scene: RenderScene,
    scene_dirty_ranges: Vec<(u32, u32)>,
    previous_dirty_ranges: Vec<(u32, u32)>,
    buckets: Vec<IndexBucket>,
    last_lods: Vec<u8>,
    index_staging: Vec<u32>,
    instance_capacity: usize,
    index_capacity: usize,
    index_buf: wgpu::Buffer,
    scene_bg: wgpu::BindGroup,
    view_bg: wgpu::BindGroup,
    shadow_array_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,
    white_ao_view: wgpu::TextureView,
    globals_bgl: wgpu::BindGroupLayout,
    globals_taa_bgl: wgpu::BindGroupLayout,
    globals_taa_bg: wgpu::BindGroup,
    previous_inst_buf: wgpu::Buffer,
    inst_buf: wgpu::Buffer,
    shadow_staging: [Vec<u32>; 2],
    cascade_mats: [Mat4; 2],
    cascade_planes: [[Vec4; 6]; 2],
    far_shadow_cache: Vec<u32>,
    far_shadow_matrix: Mat4,
    far_shadow_valid: bool,
    far_shadow_dirty: bool,
    #[cfg(test)]
    target_rebuilds: u64,
    particles: [Vec<Particle>; 2],
    part_buf: wgpu::Buffer,
    lights: Vec<Light>,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    shadow_u: [wgpu::Buffer; 2],
    shadow_ubg: [wgpu::BindGroup; 2],
    shadow_layers: [wgpu::TextureView; 2],
    post_buf: wgpu::Buffer,
    linear: wgpu::Sampler,
    targets: Targets,
    ui_buf: wgpu::Buffer,
    ui_u: wgpu::Buffer,
    ui_bg: wgpu::BindGroup,
    atlas_tex: wgpu::Texture,
    p_shadow: [wgpu::RenderPipeline; 3],
    tiles: Tiles,
    p_scene: [[wgpu::RenderPipeline; 3]; 2],
    p_base_colour: [[wgpu::RenderPipeline; 3]; 2],
    p_lighting_only: [[wgpu::RenderPipeline; 3]; 2],
    p_vm: [[wgpu::RenderPipeline; 3]; 2],
    p_base_colour_vm: [[wgpu::RenderPipeline; 3]; 2],
    p_lighting_only_vm: [[wgpu::RenderPipeline; 3]; 2],
    p_coverage: [[wgpu::RenderPipeline; 3]; 2],
    p_coverage_vm: [[wgpu::RenderPipeline; 3]; 2],
    p_sky: wgpu::RenderPipeline,
    p_glow: [[wgpu::RenderPipeline; 3]; 2],
    p_part_alpha: wgpu::RenderPipeline,
    p_part_add: wgpu::RenderPipeline,
    p_down_first: wgpu::RenderPipeline,
    p_down: wgpu::RenderPipeline,
    p_up: wgpu::RenderPipeline,
    p_comp: wgpu::RenderPipeline,
    p_debug_hdr: wgpu::RenderPipeline,
    p_debug_ao: wgpu::RenderPipeline,
    p_debug_depth: wgpu::RenderPipeline,
    p_debug_coverage: wgpu::RenderPipeline,
    p_debug_shadow: wgpu::RenderPipeline,
    debug_bindings: DebugBindings,
    p_ui: wgpu::RenderPipeline,
    cam: Camera,
    /// Unjittered view-projection of the current frame (culling, motion, GTAO).
    view_proj: Mat4,
    /// `view_proj` with the TAA jitter applied; the scene pass rasterises with this.
    jit_view_proj: Mat4,
    /// Last frame's unjittered view-projection.
    prev_view_proj: Mat4,
    jitter: Jitter,
    /// False until the first TAA resolve after a rebuild writes history.
    history_valid: bool,
    /// Scale at which the readable TAA history was written.
    history_scale: f32,
    /// TAA history slot written by the next resolve.
    taa_write: bool,
    planes: [Vec4; 6],
    /// Lighting and atmosphere, read at render time.
    pub env: Env,
    /// Post-processing controls, read at render time.
    pub post: Post,
    /// Immediate-mode 2D overlay (HUD, text).
    pub ui: Ui,
    /// Seconds of simulated time, advanced by `render`.
    pub time: f32,
    /// Counters from the most recent frame.
    pub last: FrameStats,
    asset_upload_bytes_pending: u64,
    culled: u32,
    visible_main: u32,
    visible_shadow: [u32; 2],
    timer: Option<GpuTimer>,
    screen: Option<ScreenSpace>,
    exposure_neutral: wgpu::TextureView,
    layouts: PipelineLayouts,
    scene_sm: wgpu::ShaderModule,
    post_sm: wgpu::ShaderModule,
    gtao_sm: wgpu::ShaderModule,
    denoise_sm: wgpu::ShaderModule,
    taa_sm: wgpu::ShaderModule,
    #[cfg(any(feature = "hot-reload", test))]
    shader_library: crate::shaders::ShaderLibrary,
    pipe_ctx: crate::gfx::pipelines::GfxPipeCtx,
    #[cfg(any(feature = "hot-reload", test))]
    shadow_sm: wgpu::ShaderModule,
    #[cfg(any(feature = "hot-reload", test))]
    ui_sm: wgpu::ShaderModule,
    #[cfg(any(feature = "hot-reload", test))]
    tiles_sm: wgpu::ShaderModule,
    #[cfg(feature = "hot-reload")]
    shader_watcher: crate::hot_reload::ShaderWatcher,
    #[cfg(feature = "hot-reload")]
    hot_reload_clock: std::time::Instant,
}

pub(crate) fn frustum_planes(m: &Mat4) -> [Vec4; 6] {
    let r = [m.row(0), m.row(1), m.row(2), m.row(3)];
    let mut p = [
        r[3] + r[0],
        r[3] - r[0],
        r[3] + r[1],
        r[3] - r[1],
        r[2],
        r[3] - r[2],
    ];
    for q in p.iter_mut() {
        *q /= q.truncate().length();
    }
    p
}

pub(crate) fn sphere_visible_in_planes(planes: &[Vec4; 6], center: Vec3, radius: f32) -> bool {
    planes
        .iter()
        .all(|plane| plane.truncate().dot(center) + plane.w > -radius)
}

fn far_cascade_needs_redraw(
    valid: bool,
    matrix: Mat4,
    previous_matrix: Mat4,
    instances: &[u8],
    previous_instances: &[u8],
) -> bool {
    !valid
        || matrix.to_cols_array() != previous_matrix.to_cols_array()
        || instances != previous_instances
}

impl Gfx {
    fn material_texture_view<'a>(
        &'a self,
        id: Option<TextureId>,
        default: &'a GpuDefaultTexture,
    ) -> &'a wgpu::TextureView {
        id.and_then(|id| self.textures.get(id.0))
            .map(|texture| &texture.view)
            .unwrap_or(&default.view)
    }

    /// Builds every pipeline and target for `format` at `size`. If the adapter does not
    /// support the requested MSAA count, rendering falls back to 1 sample.
    pub fn new(
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        settings: Settings,
    ) -> Result<Gfx, RenderError> {
        setup::new_gfx(gpu, format, size, settings)
    }

    /// Render-target size in pixels (the window size scaled by the render scale).
    fn internal_size(&self) -> (u32, u32) {
        let s = self.settings.render_scale;
        (
            scale_px_ceil(self.size.0, s).max(1),
            scale_px_ceil(self.size.1, s).max(1),
        )
    }

    /// Returns the complete settings that built the current checked frame plan.
    #[must_use]
    pub const fn settings(&self) -> Settings {
        self.settings
    }

    /// Returns the current quality tier.
    #[must_use]
    pub const fn tier(&self) -> crate::RenderTier {
        self.settings.tier
    }

    /// Rebuilds the checked plan and every settings-dependent resource as one update.
    ///
    /// The candidate plan is validated before any renderer state changes. On success, its MSAA
    /// selection, screen-space targets, terminal bindings, and scene pipelines replace the prior
    /// configuration together.
    pub fn set_settings(&mut self, settings: Settings) -> Result<(), RenderError> {
        let plan = crate::FramePlan::build(settings)?;
        let msaa = setup::supported_msaa(&self.adapter, &self.device, settings);
        let shadow_size_changed = settings.shadow_size != self.settings.shadow_size;
        let tier_changed = settings.tier != self.settings.tier;

        if shadow_size_changed {
            self.rebuild_shadow_targets(settings.shadow_size);
        }
        if tier_changed {
            self.material_sampler = setup::material_sampler(&self.device, settings.tier);
            self.rebuild_material_bind_groups();
        }
        self.settings = settings;
        self.plan = plan;
        self.msaa = msaa;
        self.pipe_ctx.msaa.count = msaa;
        self.pipe_ctx.taa = self.plan.contains(crate::FramePass::Taa);
        self.pipe_ctx.opaque_depth = self.plan.opaque_depth();
        self.replace_scene_pipelines();
        self.rebuild_targets();
        Ok(())
    }

    /// Turns temporal anti-aliasing on or off through [`Gfx::set_settings`].
    pub fn set_taa(&mut self, on: bool) -> Result<(), RenderError> {
        let mut settings = self.settings;
        settings.taa = on;
        self.set_settings(settings)
    }

    fn replace_scene_pipelines(&mut self) {
        let scene = pipelines::create_scene_pipelines(
            &self.device,
            &self.layouts,
            &self.scene_sm,
            &self.pipe_ctx,
        );
        self.p_scene = scene.p_scene;
        self.p_base_colour = scene.p_base_colour;
        self.p_lighting_only = scene.p_lighting_only;
        self.p_vm = scene.p_vm;
        self.p_base_colour_vm = scene.p_base_colour_vm;
        self.p_lighting_only_vm = scene.p_lighting_only_vm;
        self.p_coverage = scene.p_coverage;
        self.p_coverage_vm = scene.p_coverage_vm;
        self.p_sky = scene.p_sky;
        self.p_glow = scene.p_glow;
        self.p_part_alpha = scene.p_part_alpha;
        self.p_part_add = scene.p_part_add;
    }

    fn rebuild_material_bind_groups(&mut self) {
        let [base_default, normal_default, orm_default, emissive_default] =
            &self.default_material_textures;
        let views: Vec<[wgpu::TextureView; 4]> = self
            .materials
            .iter()
            .map(|(_, material)| {
                let [base, normal, orm, emissive] = material.textures;
                [
                    self.material_texture_view(base, base_default).clone(),
                    self.material_texture_view(normal, normal_default).clone(),
                    self.material_texture_view(orm, orm_default).clone(),
                    self.material_texture_view(emissive, emissive_default)
                        .clone(),
                ]
            })
            .collect();
        let device = &self.device;
        let layout = &self.layouts.material_bgl;
        let sampler = &self.material_sampler;
        for ((_, material), [base, normal, orm, emissive]) in self.materials.iter_mut().zip(views) {
            material.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: material.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&base),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&normal),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&orm),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&emissive),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            });
        }
    }

    fn rebuild_shadow_targets(&mut self, size: u32) {
        let (layers, array) = setup::shadow_views(&self.device, size);
        self.shadow_layers = layers;
        self.shadow_array_view = array;
        self.far_shadow_valid = false;
        self.far_shadow_dirty = true;
    }

    fn depth_view(&self, t: &Targets) -> Option<wgpu::TextureView> {
        if let Some(s) = &self.screen {
            return Some(s.depth_render_view.clone());
        }
        t.depth.clone()
    }

    /// The selected renderer view mode.
    #[must_use]
    pub const fn view_mode(&self) -> crate::ViewMode {
        self.settings.view_mode
    }

    /// The checked pass order for the current settings.
    #[must_use]
    pub fn frame_plan(&self) -> &crate::FramePlan {
        &self.plan
    }

    /// Selects a developer view through [`Gfx::set_settings`].
    pub fn set_view_mode(&mut self, view_mode: crate::ViewMode) -> Result<(), RenderError> {
        let mut settings = self.settings;
        settings.view_mode = view_mode;
        self.set_settings(settings)
    }

    /// Multisample count in use (1 if the requested count was unsupported).
    pub fn msaa(&self) -> u32 {
        self.msaa
    }

    /// Swapchain format the composite pass writes to.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Texture compression family selected from the adapter's enabled features.
    #[must_use]
    pub const fn texture_family(&self) -> GpuTextureFormatFamily {
        self.texture_family
    }

    /// Re-points every retained primitive using `old` to an uploaded replacement mesh.
    pub fn replace_mesh(&mut self, old: MeshId, new: MeshId) -> Result<(), RenderError> {
        self.scene.replace_mesh(old, new)
    }

    /// Re-points every retained primitive using `old` to an uploaded replacement material.
    pub fn replace_material(
        &mut self,
        old: MaterialId,
        new: MaterialId,
    ) -> Result<(), RenderError> {
        self.scene.replace_material(old, new)
    }

    /// Whether a retained primitive uses any mesh or material in `pack`.
    #[must_use]
    pub fn pack_resources_in_use(&self, meshes: &[MeshId], materials: &[MaterialId]) -> bool {
        meshes.iter().any(|mesh| self.scene.uses_mesh(*mesh))
            || materials
                .iter()
                .any(|material| self.scene.uses_material(*material))
    }

    /// Adds completed asset uploads to the next frame's upload statistics.
    pub fn note_asset_upload_bytes(&mut self, bytes: u64) {
        self.asset_upload_bytes_pending = self.asset_upload_bytes_pending.saturating_add(bytes);
    }

    /// Returns resident cooked mesh and texture bytes for release tests and diagnostics.
    #[must_use]
    pub fn asset_memory_bytes(&self) -> (u64, u64) {
        let meshes = self
            .meshes
            .iter()
            .fold(0u64, |bytes, (_, mesh)| bytes.saturating_add(mesh.bytes));
        let textures = self.textures.iter().fold(0u64, |bytes, (_, texture)| {
            bytes.saturating_add(texture.bytes)
        });
        (meshes, textures)
    }

    /// Uploads a procedural mesh and returns its handle.
    pub fn add_mesh(&mut self, m: &MeshBuilder) -> Result<MeshId, RenderError> {
        use wgpu::util::DeviceExt;
        let vb = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh-v"),
                contents: bytemuck::cast_slice(&m.verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let attribute_vb = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh-attributes"),
                contents: bytemuck::cast_slice(m.attributes()),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let (index_bytes, index_format) =
            if m.verts.len() < 65_536 && m.idx.iter().all(|index| u16::try_from(*index).is_ok()) {
                let indices: Vec<u16> = m
                    .idx
                    .iter()
                    .filter_map(|index| u16::try_from(*index).ok())
                    .collect();
                (
                    bytemuck::cast_slice(&indices).to_vec(),
                    wgpu::IndexFormat::Uint16,
                )
            } else {
                (
                    bytemuck::cast_slice(&m.idx).to_vec(),
                    wgpu::IndexFormat::Uint32,
                )
            };
        let ib = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh-i"),
                contents: &index_bytes,
                usage: wgpu::BufferUsages::INDEX,
            });
        let bounds = bounds_of_vertices(&m.verts, m.radius());
        let mut lods: [Option<LodRange>; MAX_LODS] = std::array::from_fn(|_| None);
        if let Some(place) = lods.get_mut(0) {
            *place = Some(LodRange {
                vb,
                attribute_vb,
                ib,
                indices: 0..to_u32(m.idx.len()),
                vertex_base: 0,
                index_format,
                screen_size_threshold: 0.0,
                format: MeshFormat::Procedural,
            });
        }
        let bytes = u64::try_from(bytemuck::cast_slice::<_, u8>(&m.verts).len())
            .unwrap_or(u64::MAX)
            .saturating_add(
                u64::try_from(bytemuck::cast_slice::<_, u8>(m.attributes()).len())
                    .unwrap_or(u64::MAX),
            )
            .saturating_add(u64::try_from(index_bytes.len()).unwrap_or(u64::MAX));
        let slot = self
            .meshes
            .insert(GpuMesh {
                lods,
                lod_count: 1,
                bytes,
            })
            .ok_or(RenderError::SceneCapacity)?;
        let mesh = MeshId::new(slot, MeshFormat::Procedural);
        self.insert_mesh_record(
            slot.index(),
            MeshRecord::new(bounds, MeshFormat::Procedural),
        )?;
        self.buckets.push(IndexBucket::new(mesh, 0, None));
        self.scene.register_mesh(mesh, m.radius());
        Ok(mesh)
    }

    /// Uploads every cooked LOD using the exact encoded streams and returns one mesh handle.
    ///
    /// Quantized positions are dequantized in the vertex stage using the per-mesh record.
    ///
    /// # Errors
    /// Returns [`RenderError::InvalidCookedMesh`] when the stream sizes or bounds are invalid.
    pub fn add_cooked_mesh(&mut self, lods: &[MeshLod]) -> Result<MeshId, RenderError> {
        use wgpu::util::DeviceExt;
        if lods.is_empty() || lods.len() > MAX_LODS {
            return Err(RenderError::InvalidCookedMesh);
        }
        let position_encoding = lods
            .first()
            .map(|lod| lod.position_encoding)
            .ok_or(RenderError::InvalidCookedMesh)?;
        let format = match position_encoding {
            PositionEncoding::Float32 => MeshFormat::CookedFloat32,
            PositionEncoding::Quantized16 => MeshFormat::CookedQuantized16,
        };
        let mut gpu_lods: [Option<LodRange>; MAX_LODS] = std::array::from_fn(|_| None);
        let bytes = lods.iter().fold(0u64, |total, lod| {
            total
                .saturating_add(u64::try_from(lod.positions.len()).unwrap_or(u64::MAX))
                .saturating_add(u64::try_from(lod.attributes.len()).unwrap_or(u64::MAX))
                .saturating_add(u64::try_from(lod.indices.len()).unwrap_or(u64::MAX))
        });
        for (index, lod) in lods.iter().enumerate() {
            validate_cooked_lod(lod)?;
            if lod.position_encoding != position_encoding {
                return Err(RenderError::InvalidCookedMesh);
            }
            let vb = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cooked-mesh-positions"),
                    contents: &lod.positions,
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let attribute_vb = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cooked-mesh-attributes"),
                    contents: &lod.attributes,
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let ib = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cooked-mesh-indices"),
                    contents: &lod.indices,
                    usage: wgpu::BufferUsages::INDEX,
                });
            let index_format = match lod.index_encoding {
                IndexEncoding::U16 => wgpu::IndexFormat::Uint16,
                IndexEncoding::U32 => wgpu::IndexFormat::Uint32,
            };
            let range = LodRange {
                vb,
                attribute_vb,
                ib,
                indices: 0..lod.index_count,
                vertex_base: 0,
                index_format,
                screen_size_threshold: lod.screen_size_threshold,
                format,
            };
            if let Some(place) = gpu_lods.get_mut(index) {
                *place = Some(range);
            }
        }
        let first_lod = lods.first().ok_or(RenderError::InvalidCookedMesh)?;
        let slot = self
            .meshes
            .insert(GpuMesh {
                lods: gpu_lods,
                lod_count: lods.len(),
                bytes,
            })
            .ok_or(RenderError::SceneCapacity)?;
        self.insert_mesh_record(slot.index(), MeshRecord::new(first_lod.bounds, format))?;
        let mesh = MeshId::new(slot, format);
        self.buckets.push(IndexBucket::new(mesh, 0, None));
        self.scene.register_mesh(
            mesh,
            lods.iter()
                .map(|lod| lod.bounds.radius)
                .fold(0.0f32, f32::max),
        );
        Ok(mesh)
    }

    fn insert_mesh_record(&mut self, index: u32, record: MeshRecord) -> Result<(), RenderError> {
        let index = usize::try_from(index).map_err(|_| RenderError::SceneCapacity)?;
        let required = self.mesh_records.len().max(index.saturating_add(1));
        if required > self.mesh_record_capacity {
            let capacity = self
                .mesh_record_capacity
                .saturating_mul(2)
                .max(required)
                .max(1);
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mesh-records"),
                size: u64::try_from(capacity)
                    .unwrap_or(u64::MAX)
                    .saturating_mul(std::mem::size_of::<MeshRecord>() as u64),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            if !self.mesh_records.is_empty() {
                self.queue
                    .write_buffer(&buffer, 0, bytemuck::cast_slice(&self.mesh_records));
            }
            self.mesh_records_buf = buffer;
            self.mesh_record_capacity = capacity;
            self.rebuild_scene_bind_group();
            self.rebuild_view_bind_group();
        }
        self.mesh_records.resize(required, MeshRecord::default());
        let place = self
            .mesh_records
            .get_mut(index)
            .ok_or(RenderError::SceneCapacity)?;
        *place = record;
        let offset = u64::try_from(index)
            .unwrap_or(u64::MAX)
            .saturating_mul(std::mem::size_of::<MeshRecord>() as u64);
        self.queue
            .write_buffer(&self.mesh_records_buf, offset, bytemuck::bytes_of(&record));
        Ok(())
    }

    fn rebuild_scene_bind_group(&mut self) {
        self.scene_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene-instances"),
            layout: &self.layouts.scene_instances_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.inst_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.index_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.previous_inst_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.mesh_records_buf.as_entire_binding(),
                },
            ],
        });
    }

    fn rebuild_view_bind_group(&mut self) {
        let ao_view = self
            .screen
            .as_ref()
            .map(|screen| &screen.ao_final_view)
            .unwrap_or(&self.white_ao_view);
        self.view_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene-view"),
            layout: &self.layouts.view_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.inst_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.index_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.previous_inst_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.mesh_records_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.tiles.read_buffer().as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.shadow_array_view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(ao_view),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::Sampler(&self.linear),
                },
            ],
        });
    }

    /// Frees a mesh's GPU buffers after ensuring no retained primitive references it.
    ///
    /// Mesh slots are not reused, so old identifiers stay invalid after removal.
    ///
    /// # Errors
    /// Returns [`RenderError::UnknownMesh`] for a stale identifier and
    /// [`RenderError::MeshInUse`] while a live primitive still references the mesh.
    pub fn remove_mesh(&mut self, mesh: MeshId) -> Result<(), RenderError> {
        if self.scene.uses_mesh(mesh) {
            return Err(RenderError::MeshInUse(mesh));
        }
        self.scene.unregister_mesh(mesh)?;
        self.meshes
            .remove(mesh.0)
            .ok_or(RenderError::UnknownMesh(mesh))?;
        Ok(())
    }

    /// Uploads worker-prepared mip data without transcoding on the render thread.
    ///
    /// The caller owns the prepared CPU data until this frame-boundary upload completes.
    pub fn add_texture(&mut self, prepared: PreparedTexture) -> Result<TextureId, RenderError> {
        use wgpu::util::DeviceExt;
        if prepared.family != self.texture_family {
            return Err(RenderError::TextureFamilyMismatch);
        }
        let base_mip = prepared.mips.first().ok_or(RenderError::TextureUpload(
            engine_assets::format::AssetError::Structure,
        ))?;
        let mip_count = u32::try_from(prepared.mips.len()).map_err(|_| {
            RenderError::TextureUpload(engine_assets::format::AssetError::Structure)
        })?;
        let format = texture_format(prepared.family, prepared.colour_space);
        let mut upload_bytes = Vec::new();
        for mip in &prepared.mips {
            upload_bytes.extend_from_slice(&mip.bytes);
        }
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("asset-texture"),
                size: wgpu::Extent3d {
                    width: base_mip.width,
                    height: base_mip.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: mip_count,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::MipMajor,
            &upload_bytes,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let slot = self
            .textures
            .insert(GpuTexture {
                _texture: texture,
                view,
                family: prepared.family,
                bytes: u64::try_from(upload_bytes.len()).unwrap_or(u64::MAX),
                material_uses: 0,
            })
            .ok_or(RenderError::SceneCapacity)?;
        Ok(TextureId(slot))
    }

    /// Creates a shared PBR material bind group from factors and optional texture handles.
    ///
    /// Missing maps use the renderer's shared white, flat-normal, ORM, or black defaults.
    ///
    /// # Errors
    /// Returns [`RenderError::InvalidMaterialDesc`] for invalid factors and
    /// [`RenderError::UnknownTexture`] for a stale texture handle.
    pub fn add_material(&mut self, desc: MaterialDesc) -> Result<MaterialId, RenderError> {
        use wgpu::util::DeviceExt;

        let scalar_factors = [
            desc.metallic,
            desc.roughness,
            desc.emissive_strength,
            desc.normal_scale,
        ];
        let alpha_cutoff = match desc.alpha_mode {
            AlphaMode::Opaque => 0.0,
            AlphaMode::Mask(cutoff) => cutoff,
        };
        if desc
            .base_color
            .iter()
            .chain(scalar_factors.iter())
            .any(|value| !value.is_finite())
            || !(0.0..=1.0).contains(&desc.metallic)
            || !(0.0..=1.0).contains(&desc.roughness)
            || desc.emissive_strength < 0.0
            || desc.normal_scale < 0.0
            || !alpha_cutoff.is_finite()
            || !(0.0..=1.0).contains(&alpha_cutoff)
        {
            return Err(RenderError::InvalidMaterialDesc);
        }
        let textures = [
            desc.base_color_texture,
            desc.normal_texture,
            desc.orm_texture,
            desc.emissive_texture,
        ];
        for id in textures.iter().flatten() {
            let Some(texture) = self.textures.get(id.0) else {
                return Err(RenderError::UnknownTexture(*id));
            };
            if texture.material_uses.checked_add(1).is_none() {
                return Err(RenderError::TextureUseCountOverflow(*id));
            }
        }
        for (index, id) in textures.iter().flatten().enumerate() {
            if textures
                .iter()
                .take(index)
                .flatten()
                .any(|previous| previous == id)
            {
                continue;
            }
            let Some(texture) = self.textures.get(id.0) else {
                return Err(RenderError::UnknownTexture(*id));
            };
            let references = textures
                .iter()
                .flatten()
                .filter(|reference| *reference == id)
                .count();
            let total_uses = u32::try_from(references).unwrap_or(u32::MAX);
            if texture.material_uses.checked_add(total_uses).is_none() {
                return Err(RenderError::TextureUseCountOverflow(*id));
            }
        }
        let uniform = MaterialUniform {
            base_color: desc.base_color,
            factors: scalar_factors,
            alpha: [
                f32::from(matches!(desc.alpha_mode, AlphaMode::Mask(_))),
                alpha_cutoff,
                0.0,
                0.0,
            ],
        };
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("material-uniform"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let [base_texture, normal_texture, orm_texture, emissive_texture] = textures;
        let [base_default, normal_default, orm_default, emissive_default] =
            &self.default_material_textures;
        let [base_view, normal_view, orm_view, emissive_view] = [
            self.material_texture_view(base_texture, base_default),
            self.material_texture_view(normal_texture, normal_default),
            self.material_texture_view(orm_texture, orm_default),
            self.material_texture_view(emissive_texture, emissive_default),
        ];
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material"),
            layout: &self.layouts.material_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(base_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(normal_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(orm_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(emissive_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self.material_sampler),
                },
            ],
        });
        let slot = self
            .materials
            .insert(GpuMaterial {
                uniform: buffer,
                bind_group,
                textures,
            })
            .ok_or(RenderError::SceneCapacity)?;
        let id = MaterialId(slot);
        for texture_id in textures.iter().flatten() {
            let Some(texture) = self.textures.get_mut(texture_id.0) else {
                return Err(RenderError::UnknownTexture(*texture_id));
            };
            texture.material_uses += 1;
        }
        self.scene.register_material(id);
        Ok(id)
    }

    /// Frees a material bind group after ensuring no retained primitive references it.
    ///
    /// # Errors
    /// Returns [`RenderError::UnknownMaterial`] for a stale handle and
    /// [`RenderError::MaterialInUse`] while a live primitive references it.
    pub fn remove_material(&mut self, id: MaterialId) -> Result<(), RenderError> {
        if self.scene.uses_material(id) {
            return Err(RenderError::MaterialInUse(id));
        }
        let material = self
            .materials
            .remove(id.0)
            .ok_or(RenderError::UnknownMaterial(id))?;
        for texture_id in material.textures.iter().flatten() {
            let Some(texture) = self.textures.get_mut(texture_id.0) else {
                continue;
            };
            texture.material_uses = texture.material_uses.saturating_sub(1);
        }
        self.scene.unregister_material(id)?;
        Ok(())
    }

    /// Frees an uploaded texture.
    ///
    /// # Errors
    /// Returns [`RenderError::UnknownTexture`] for a stale or invalid handle.
    pub fn remove_texture(&mut self, id: TextureId) -> Result<(), RenderError> {
        if self
            .textures
            .get(id.0)
            .is_some_and(|texture| texture.material_uses > 0)
        {
            return Err(RenderError::TextureInUse(id));
        }
        self.textures
            .remove(id.0)
            .ok_or(RenderError::UnknownTexture(id))?;
        Ok(())
    }

    fn texture_bytes_by_family(&self) -> [u64; 4] {
        let mut totals = [0u64; 4];
        for (_, texture) in self.textures.iter() {
            let family_index = match texture.family {
                GpuTextureFormatFamily::Bc7 => 0,
                GpuTextureFormatFamily::Astc4x4 => 1,
                GpuTextureFormatFamily::Etc2 => 2,
                GpuTextureFormatFamily::Rgba8 => 3,
            };
            if let Some(total) = totals.get_mut(family_index) {
                *total = total.saturating_add(texture.bytes);
            }
        }
        totals
    }

    /// Mutable access to the retained render scene.
    #[must_use]
    pub fn scene_mut(&mut self) -> &mut RenderScene {
        &mut self.scene
    }

    fn submit_scene(&mut self) {
        if self.last_lods.len() < self.scene.len_slots() {
            self.last_lods.resize(self.scene.len_slots(), 0);
        }
        for bucket in &mut self.buckets {
            bucket.shadow[0].clear();
            bucket.shadow[1].clear();
            bucket.both.clear();
            bucket.main_only.clear();
            bucket.vm.clear();
            bucket.glow.clear();
        }
        let stats = self.scene.cull(
            &self.planes,
            &self.cascade_planes,
            |mesh,
             material,
             slot_id,
             layer,
             main_visible,
             casts_shadow,
             shadow_visible,
             center,
             radius| {
                let Some(gpu_mesh) = self.meshes.get(mesh.0) else {
                    return;
                };
                let Some(previous) = usize::try_from(slot_id)
                    .ok()
                    .and_then(|slot| self.last_lods.get(slot).copied())
                else {
                    return;
                };
                let distance = (center - self.cam.pos).length().max(self.cam.near);
                let screen_size = radius / (distance * (self.cam.fov_y * 0.5).tan());
                let thresholds = gpu_mesh.thresholds();
                let lod = crate::lod::select_lod(
                    screen_size,
                    thresholds.get(..gpu_mesh.lod_count).unwrap_or(&[]),
                    usize::from(previous),
                );
                if let Ok(slot) = usize::try_from(slot_id)
                    && let Some(last_lod) = self.last_lods.get_mut(slot)
                    && let Ok(lod_value) = u8::try_from(lod)
                {
                    *last_lod = lod_value;
                }
                let material_id = match material {
                    Material::Procedural(_) => None,
                    Material::Textured(id) => Some(id),
                };
                match layer {
                    crate::Layer::ViewModel => {
                        let bucket = ensure_index_bucket(
                            &mut self.buckets,
                            mesh,
                            lod,
                            material_id,
                            self.instance_capacity,
                        );
                        if let Some(bucket) = self.buckets.get_mut(bucket) {
                            bucket.vm.push(slot_id);
                        }
                    }
                    crate::Layer::Glow => {
                        let bucket = ensure_index_bucket(
                            &mut self.buckets,
                            mesh,
                            lod,
                            material_id,
                            self.instance_capacity,
                        );
                        if let Some(bucket) = self.buckets.get_mut(bucket) {
                            bucket.glow.push(slot_id);
                        }
                    }
                    crate::Layer::World => {
                        let shadow_lod = crate::lod::shadow_lod(lod, gpu_mesh.lod_count);
                        let shadow_bucket = ensure_index_bucket(
                            &mut self.buckets,
                            mesh,
                            shadow_lod,
                            material_id,
                            self.instance_capacity,
                        );
                        for (cascade, is_visible) in shadow_visible.into_iter().enumerate() {
                            if is_visible
                                && let Some(bucket) = self.buckets.get_mut(shadow_bucket)
                                && let Some(indices) = bucket.shadow.get_mut(cascade)
                            {
                                indices.push(slot_id);
                            }
                        }
                        if main_visible {
                            let main_bucket = ensure_index_bucket(
                                &mut self.buckets,
                                mesh,
                                lod,
                                material_id,
                                self.instance_capacity,
                            );
                            let Some(bucket) = self.buckets.get_mut(main_bucket) else {
                                return;
                            };
                            if casts_shadow {
                                bucket.both.push(slot_id);
                            } else {
                                bucket.main_only.push(slot_id);
                            }
                        }
                    }
                }
            },
        );
        self.buckets.sort_unstable_by_key(|bucket| {
            (
                bucket.mesh.format().pipeline_index(),
                u8::from(bucket.material.is_some()),
                bucket.material.map_or(0, MaterialId::index),
                bucket.mesh.index(),
                bucket.lod,
            )
        });
        self.culled = stats.culled;
        self.visible_main = stats.visible_main;
        self.visible_shadow = stats.visible_shadow;
    }

    fn ensure_scene_gpu_capacity(&mut self) {
        if self.scene.len_slots() <= self.instance_capacity {
            return;
        }
        let mut next = self.instance_capacity.max(1);
        while next < self.scene.len_slots() {
            next = next.saturating_mul(2);
        }
        let next_indices = next.saturating_mul(INDEXES_PER_PRIMITIVE);
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        self.inst_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: u64::try_from(next.saturating_mul(std::mem::size_of::<Instance>()))
                .unwrap_or(u64::MAX),
            usage: storage,
            mapped_at_creation: false,
        });
        self.previous_inst_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("previous-instances"),
            size: u64::try_from(next.saturating_mul(std::mem::size_of::<PreviousTransform>()))
                .unwrap_or(u64::MAX),
            usage: storage,
            mapped_at_creation: false,
        });
        self.index_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("visible-slot-indices"),
            size: u64::try_from(next_indices.saturating_mul(std::mem::size_of::<u32>()))
                .unwrap_or(u64::MAX),
            usage: storage,
            mapped_at_creation: false,
        });
        self.globals_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals-and-scene"),
            layout: &self.globals_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.inst_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.index_buf.as_entire_binding(),
                },
            ],
        });
        self.globals_taa_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals-scene-taa"),
            layout: &self.globals_taa_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.inst_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.index_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.previous_inst_buf.as_entire_binding(),
                },
            ],
        });
        self.rebuild_scene_bind_group();
        self.rebuild_view_bind_group();
        for bucket in &mut self.buckets {
            bucket.reserve_slots(next);
        }
        self.index_staging
            .reserve(next_indices.saturating_sub(self.index_staging.len()));
        self.instance_capacity = next;
        self.index_capacity = next_indices;
    }

    fn flatten_visible_indices(&mut self) {
        self.index_staging.clear();
        for staging in &mut self.shadow_staging {
            staging.clear();
        }
        for bucket in &mut self.buckets {
            let ranges = &mut bucket.ranges;
            for cascade in 0..2 {
                if let Some(start) = ranges.shadow_start.get_mut(cascade) {
                    *start = to_u32(self.index_staging.len());
                }
                if let Some(indices) = bucket.shadow.get(cascade) {
                    if let Some(count) = ranges.shadow_count.get_mut(cascade) {
                        *count = to_u32(indices.len());
                    }
                    if let Some(staging) = self.shadow_staging.get_mut(cascade) {
                        staging.extend_from_slice(indices);
                    }
                    self.index_staging.extend_from_slice(indices);
                }
            }
            ranges.start = to_u32(self.index_staging.len());
            ranges.both = to_u32(bucket.both.len());
            ranges.mo = to_u32(bucket.main_only.len());
            self.index_staging.extend_from_slice(&bucket.both);
            self.index_staging.extend_from_slice(&bucket.main_only);
            ranges.vm_start = to_u32(self.index_staging.len());
            ranges.vm = to_u32(bucket.vm.len());
            self.index_staging.extend_from_slice(&bucket.vm);
            ranges.glow_start = to_u32(self.index_staging.len());
            ranges.glow = to_u32(bucket.glow.len());
            self.index_staging.extend_from_slice(&bucket.glow);
        }
    }

    fn upload_scene_changes(&mut self) -> u64 {
        self.scene.build_dirty_ranges(&mut self.scene_dirty_ranges);
        self.scene
            .build_previous_dirty_ranges(&mut self.previous_dirty_ranges);
        let mut uploaded = 0u64;
        for &(start, end) in &self.scene_dirty_ranges {
            if let (Ok(start_index), Ok(end_index)) = (usize::try_from(start), usize::try_from(end))
                && let Some(records) = self.scene.record_range(start_index..end_index)
            {
                let offset =
                    u64::from(start).saturating_mul(std::mem::size_of::<Instance>() as u64);
                self.queue
                    .write_buffer(&self.inst_buf, offset, bytemuck::cast_slice(records));
                uploaded = uploaded.saturating_add(
                    u64::try_from(std::mem::size_of_val(records)).unwrap_or(u64::MAX),
                );
            }
        }
        if self.plan.contains(crate::FramePass::Taa) {
            for &(start, end) in &self.previous_dirty_ranges {
                if let (Ok(start_index), Ok(end_index)) =
                    (usize::try_from(start), usize::try_from(end))
                    && let Some(records) = self.scene.previous_record_range(start_index..end_index)
                {
                    let offset = u64::from(start)
                        .saturating_mul(std::mem::size_of::<PreviousTransform>() as u64);
                    self.queue.write_buffer(
                        &self.previous_inst_buf,
                        offset,
                        bytemuck::cast_slice(records),
                    );
                    uploaded = uploaded.saturating_add(
                        u64::try_from(std::mem::size_of_val(records)).unwrap_or(u64::MAX),
                    );
                }
            }
        }
        self.scene.clear_dirty();
        uploaded
    }

    /// Resizes the window targets. Zero-sized and unchanged sizes are ignored.
    pub fn resize(&mut self, size: (u32, u32)) {
        if size == self.size || size.0 == 0 || size.1 == 0 {
            return;
        }
        self.size = size;
        self.ui.size = glam::Vec2::new(size.0 as f32, size.1 as f32);
        self.rebuild_targets();
    }

    /// The internal resolution scale in effect, as last set with [`Gfx::set_render_scale`] or by
    /// the settings.
    #[must_use]
    pub fn render_scale(&self) -> f32 {
        self.settings.render_scale
    }

    /// Changes the internal resolution scale (clamped to 0.4 to 1.0) without rebuilding targets.
    pub fn set_render_scale(&mut self, s: f32) {
        self.settings.render_scale = s.clamp(0.4, 1.0);
        let (width, height) = self.internal_size();
        self.tiles.set_viewport((width, height));
    }

    fn rebuild_targets(&mut self) {
        #[cfg(test)]
        {
            self.target_rebuilds = self.target_rebuilds.saturating_add(1);
        }
        let features = self.settings.features();
        self.history_valid = false;
        if self.plan.reuses_prepass_depth() {
            self.screen = Some(screen_space::create(&screen_space::ScreenSpaceDesc {
                device: &self.device,
                features,
                scene_shader: &self.scene_sm,
                post_shader: &self.post_sm,
                gtao_shader: &self.gtao_sm,
                denoise_shader: &self.denoise_sm,
                globals_taa_bgl: &self.layouts.globals_taa_bgl,
                prepass_layout: &self.layouts.scene_layout,
                post_bgl: &self.layouts.post_bgl,
                linear: &self.linear,
                window_size: self.size,
                taa: features.taa(),
                taa_history_format: self.taa_history_format,
                taa_shader: &self.taa_sm,
            }));
        } else {
            self.screen = None;
        }
        let exp = self
            .screen
            .as_ref()
            .map(|s| &s.exposure_view)
            .unwrap_or(&self.exposure_neutral);
        let taa = self.screen.as_ref().and_then(|s| {
            let pipe = s.taa.as_ref()?;
            Some(TaaTargetDesc {
                bgl: &pipe.bgl,
                velocity: &s.velocity_view,
                depth: &s.depth_sample_view,
            })
        });
        self.targets = targets::build(&FrameTargetDesc {
            device: &self.device,
            post_bgl: &self.layouts.post_bgl,
            sampler: &self.linear,
            post_buf: &self.post_buf,
            window_size: self.size,
            msaa: self.msaa,
            skip_depth: features.prepass_enabled(),
            exposure: exp,
            measure_exposure: &self.exposure_neutral,
            taa_history_format: self.taa_history_format,
            taa,
        });
        self.debug_bindings = setup::debug_bindings(
            &self.device,
            &self.layouts,
            &self.targets,
            self.screen.as_ref(),
            &self.shadow_array_view,
            matches!(
                self.settings.view_mode,
                crate::ViewMode::OpaqueCoverage { .. }
            ),
        );
        self.tiles.resize(&self.device, self.size);
        self.tiles.set_viewport(self.internal_size());
        self.rebuild_view_bind_group();
    }

    /// Start a frame: sets the camera used for culling and clears all submissions.
    pub fn begin_frame(&mut self, cam: Camera) {
        self.scene.advance_frame();
        #[cfg(feature = "hot-reload")]
        self.poll_shader_hot_reload(std::time::Instant::now());
        self.cam = cam;
        let (cascade_mats, _) = self.cascades();
        self.cascade_mats = cascade_mats;
        self.cascade_planes = [
            frustum_planes(&cascade_mats[0]),
            frustum_planes(&cascade_mats[1]),
        ];
        let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
        let proj = Mat4::perspective_rh(cam.fov_y, aspect, cam.near, cam.far);
        self.prev_view_proj = self.view_proj;
        self.view_proj = proj * cam.view();
        self.jit_view_proj = if self.plan.contains(crate::FramePass::Taa) {
            let (w, h) = self.internal_size();
            let ndc = Jitter::offset_ndc(self.jitter.next_sample(), w, h);
            jittered_projection(proj, ndc) * cam.view()
        } else {
            self.view_proj
        };
        self.planes = frustum_planes(&self.view_proj);
        for b in &mut self.buckets {
            b.shadow[0].clear();
            b.shadow[1].clear();
            b.both.clear();
            b.main_only.clear();
            b.vm.clear();
            b.glow.clear();
        }
        self.shadow_staging[0].clear();
        self.shadow_staging[1].clear();
        self.particles[0].clear();
        self.particles[1].clear();
        self.lights.clear();
        self.ui.clear();
        self.culled = 0;
    }

    /// GPU time per stage from the latest completed measurement; `None` without timestamp support.
    pub fn gpu_times(&self) -> Option<GpuTimes> {
        self.timer.as_ref().map(|t| t.times)
    }

    /// The camera set by the last `begin_frame`.
    pub fn camera(&self) -> &Camera {
        &self.cam
    }

    fn visible(&self, c: Vec3, r: f32) -> bool {
        sphere_visible_in_planes(&self.planes, c, r)
    }

    /// Additive (fire, sparks, muzzle) when `additive`, else premultiplied alpha (smoke).
    #[inline]
    pub fn particle(&mut self, p: Particle, additive: bool) {
        let vis = self.visible(Vec3::from(p.pos), p.size + Vec3::from(p.vel).length());
        let v = if additive {
            &mut self.particles[1]
        } else {
            &mut self.particles[0]
        };
        if vis && v.len() < MAX_PARTICLES / 2 {
            v.push(p);
        }
    }

    /// Point light for this frame. Lights beyond the tier's budget are dropped
    /// (callers submit the most important first).
    pub fn light(&mut self, pos: Vec3, radius: f32, color: [f32; 3], intensity: f32) {
        if self.lights.len() < (self.settings.max_lights as usize).min(MAX_LIGHTS)
            && self.visible(pos, radius)
        {
            self.lights.push(Light {
                pos_r: [pos.x, pos.y, pos.z, radius],
                col_i: [color[0], color[1], color[2], intensity],
            });
        }
    }

    fn cascades(&self) -> ([Mat4; 2], [f32; 2]) {
        let cam = &self.cam;
        let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
        let t = (cam.fov_y * 0.5).tan();
        let f = cam.forward();
        let sun = self.env.sun_dir.normalize();
        let up = if sun.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        let ss = self.settings.shadow_size as f32;
        let mut out = [Mat4::IDENTITY; 2];
        let mut texel = [0.0; 2];
        let mut near = cam.near;
        for ((o, tx), &far) in out
            .iter_mut()
            .zip(texel.iter_mut())
            .zip(self.settings.splits.iter())
        {
            // Bounding sphere of the slice: constant radius, so the map never swims.
            let mid = (near + far) * 0.5;
            let cf = Vec3::new(far * t * aspect, far * t, far);
            let cn = Vec3::new(near * t * aspect, near * t, near);
            let c = Vec3::new(0.0, 0.0, mid);
            let r = ((cf - c).length().max((cn - c).length()) + 1.0).ceil();
            let center = cam.pos + f * mid;
            let depth = r * 2.0 + 80.0;
            let lv = Mat4::look_at_rh(center + sun * (r + 80.0), center, up);
            let wt = 2.0 * r / ss;
            let pc = lv.transform_point3(center);
            let snap = Vec3::new(
                (pc.x / wt).round() * wt - pc.x,
                (pc.y / wt).round() * wt - pc.y,
                0.0,
            );
            let lv = Mat4::from_translation(snap) * lv;
            *o = Mat4::orthographic_rh(-r, r, -r, r, 0.1, depth) * lv;
            *tx = wt;
            near = far;
        }
        (out, texel)
    }
}

fn ensure_index_bucket(
    buckets: &mut Vec<IndexBucket>,
    mesh: MeshId,
    lod: usize,
    material: Option<MaterialId>,
    capacity: usize,
) -> usize {
    if let Some(index) = buckets
        .iter()
        .position(|bucket| bucket.mesh == mesh && bucket.lod == lod && bucket.material == material)
    {
        return index;
    }
    let mut bucket = IndexBucket::new(mesh, lod, material);
    bucket.reserve_slots(capacity);
    buckets.push(bucket);
    buckets.len().saturating_sub(1)
}

fn bounds_of_vertices(vertices: &[crate::Vertex], radius: f32) -> Bounds {
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for vertex in vertices {
        let position = Vec3::from_array(vertex.pos);
        minimum = minimum.min(position);
        maximum = maximum.max(position);
    }
    Bounds {
        min: minimum.to_array(),
        max: maximum.to_array(),
        center: [0.0; 3],
        radius,
    }
}

fn valid_mesh_bounds(bounds: Bounds) -> bool {
    bounds
        .min
        .iter()
        .chain(bounds.max.iter())
        .chain(bounds.center.iter())
        .all(|value| value.is_finite())
        && bounds.radius.is_finite()
        && bounds.radius >= 0.0
        && bounds
            .min
            .iter()
            .zip(bounds.max.iter())
            .all(|(minimum, maximum)| minimum <= maximum)
}

fn validate_cooked_lod(lod: &MeshLod) -> Result<(), RenderError> {
    let position_stride = match lod.position_encoding {
        PositionEncoding::Float32 => 12usize,
        PositionEncoding::Quantized16 => 8usize,
    };
    let index_stride = match lod.index_encoding {
        IndexEncoding::U16 => 2usize,
        IndexEncoding::U32 => 4usize,
    };
    let vertex_count =
        usize::try_from(lod.vertex_count).map_err(|_| RenderError::InvalidCookedMesh)?;
    let index_count =
        usize::try_from(lod.index_count).map_err(|_| RenderError::InvalidCookedMesh)?;
    let expected_positions = vertex_count
        .checked_mul(position_stride)
        .ok_or(RenderError::InvalidCookedMesh)?;
    let expected_attributes = vertex_count
        .checked_mul(12)
        .ok_or(RenderError::InvalidCookedMesh)?;
    let expected_indices = index_count
        .checked_mul(index_stride)
        .ok_or(RenderError::InvalidCookedMesh)?;
    if vertex_count == 0
        || lod.index_count < 3
        || !lod.index_count.is_multiple_of(3)
        || lod.positions.len() != expected_positions
        || lod.attributes.len() != expected_attributes
        || lod.indices.len() != expected_indices
        || !valid_mesh_bounds(lod.bounds)
        || !lod.screen_size_threshold.is_finite()
        || lod.screen_size_threshold < 0.0
        || !lod.screen_error.is_finite()
        || lod.screen_error < 0.0
        || !valid_cooked_indices(lod)
    {
        return Err(RenderError::InvalidCookedMesh);
    }
    Ok(())
}

fn valid_cooked_indices(lod: &MeshLod) -> bool {
    let vertices = usize::try_from(lod.vertex_count).unwrap_or(usize::MAX);
    match lod.index_encoding {
        IndexEncoding::U16 => lod
            .indices
            .as_chunks::<2>()
            .0
            .iter()
            .all(|bytes| usize::from(u16::from_le_bytes(*bytes)) < vertices),
        IndexEncoding::U32 => lod.indices.as_chunks::<4>().0.iter().all(|bytes| {
            usize::try_from(u32::from_le_bytes(*bytes)).is_ok_and(|index| index < vertices)
        }),
    }
}

fn texture_format(
    family: GpuTextureFormatFamily,
    colour_space: ColourSpace,
) -> wgpu::TextureFormat {
    match (family, colour_space) {
        (GpuTextureFormatFamily::Bc7, ColourSpace::Srgb) => wgpu::TextureFormat::Bc7RgbaUnormSrgb,
        (GpuTextureFormatFamily::Bc7, ColourSpace::Linear) => wgpu::TextureFormat::Bc7RgbaUnorm,
        (GpuTextureFormatFamily::Astc4x4, ColourSpace::Srgb) => wgpu::TextureFormat::Astc {
            block: wgpu::AstcBlock::B4x4,
            channel: wgpu::AstcChannel::UnormSrgb,
        },
        (GpuTextureFormatFamily::Astc4x4, ColourSpace::Linear) => wgpu::TextureFormat::Astc {
            block: wgpu::AstcBlock::B4x4,
            channel: wgpu::AstcChannel::Unorm,
        },
        (GpuTextureFormatFamily::Etc2, ColourSpace::Srgb) => {
            wgpu::TextureFormat::Etc2Rgba8UnormSrgb
        }
        (GpuTextureFormatFamily::Etc2, ColourSpace::Linear) => wgpu::TextureFormat::Etc2Rgba8Unorm,
        (GpuTextureFormatFamily::Rgba8, ColourSpace::Srgb) => wgpu::TextureFormat::Rgba8UnormSrgb,
        (GpuTextureFormatFamily::Rgba8, ColourSpace::Linear) => wgpu::TextureFormat::Rgba8Unorm,
    }
}

#[cfg(test)]
mod asset_texture_tests {
    use super::*;

    #[test]
    fn colour_space_selects_linear_or_srgb_gpu_variants() {
        assert_eq!(
            texture_format(GpuTextureFormatFamily::Bc7, ColourSpace::Srgb),
            wgpu::TextureFormat::Bc7RgbaUnormSrgb
        );
        assert_eq!(
            texture_format(GpuTextureFormatFamily::Bc7, ColourSpace::Linear),
            wgpu::TextureFormat::Bc7RgbaUnorm
        );
        assert_eq!(
            texture_format(GpuTextureFormatFamily::Astc4x4, ColourSpace::Srgb),
            wgpu::TextureFormat::Astc {
                block: wgpu::AstcBlock::B4x4,
                channel: wgpu::AstcChannel::UnormSrgb,
            }
        );
        assert_eq!(
            texture_format(GpuTextureFormatFamily::Etc2, ColourSpace::Linear),
            wgpu::TextureFormat::Etc2Rgba8Unorm
        );
        assert_eq!(
            texture_format(GpuTextureFormatFamily::Rgba8, ColourSpace::Srgb),
            wgpu::TextureFormat::Rgba8UnormSrgb
        );
    }
}

#[cfg(test)]
mod shadow_tests {
    use super::{Gfx, PostU, far_cascade_needs_redraw, frustum_planes, sphere_visible_in_planes};
    use crate::screen_space::{FullscreenOptions, ScreenSpace};
    use crate::setup::resource_creation_counts;
    use crate::{FramePass, RenderTier, ViewMode};
    use glam::{Mat4, Vec3};

    #[test]
    fn sphere_intersection_keeps_inside_and_rejects_outside() {
        let planes = frustum_planes(&Mat4::orthographic_rh(-5.0, 5.0, -5.0, 5.0, 0.1, 20.0));
        assert!(sphere_visible_in_planes(&planes, Vec3::ZERO, 0.5));
        assert!(!sphere_visible_in_planes(
            &planes,
            Vec3::new(8.0, 0.0, 0.0),
            0.5
        ));
    }

    #[test]
    fn caster_behind_camera_is_kept_when_in_light_box() {
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 20.0), Vec3::ZERO, Vec3::Y);
        let light_box = Mat4::orthographic_rh(-10.0, 10.0, -10.0, 10.0, 0.1, 40.0) * view;
        let planes = frustum_planes(&light_box);
        assert!(sphere_visible_in_planes(
            &planes,
            Vec3::new(0.0, 0.0, 2.0),
            0.5
        ));
    }

    #[test]
    fn far_object_is_outside_near_cascade_but_inside_far_cascade() {
        let near = frustum_planes(&Mat4::orthographic_rh(-5.0, 5.0, -5.0, 5.0, 0.1, 20.0));
        let far = frustum_planes(&Mat4::orthographic_rh(-30.0, 30.0, -30.0, 30.0, 0.1, 80.0));
        let object = Vec3::new(18.0, 0.0, -10.0);
        assert!(!sphere_visible_in_planes(&near, object, 1.0));
        assert!(sphere_visible_in_planes(&far, object, 1.0));
    }

    #[test]
    fn far_cascade_cache_invalidates_for_box_or_instance_changes() {
        let matrix = Mat4::IDENTITY;
        let moved_box = Mat4::from_translation(Vec3::X);
        assert!(!far_cascade_needs_redraw(
            true,
            matrix,
            matrix,
            &[1, 2],
            &[1, 2]
        ));
        assert!(far_cascade_needs_redraw(false, matrix, matrix, &[], &[]));
        assert!(far_cascade_needs_redraw(true, moved_box, matrix, &[], &[]));
        assert!(far_cascade_needs_redraw(
            true,
            matrix,
            matrix,
            &[1],
            &[1, 2]
        ));
        assert!(far_cascade_needs_redraw(
            true,
            matrix,
            matrix,
            &[1, 3],
            &[1, 2]
        ));
    }

    #[test]
    fn render_scale_changes_keep_targets_and_taa_history() {
        let gpu = match platform::Gpu::new(wgpu::Instance::default(), None) {
            Ok(gpu) => gpu,
            Err(error) => {
                eprintln!("skipping dynamic-resolution GPU check: {error}");
                return;
            }
        };
        let mut settings = RenderTier::High.settings();
        settings.taa = true;
        let created = Gfx::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, (128, 128), settings);
        assert!(
            created.is_ok(),
            "GPU adapter could not create render targets"
        );
        let Some(mut gfx) = created.ok() else {
            return;
        };
        gfx.history_valid = true;
        let builds = gfx.target_rebuilds;
        let creations = resource_creation_counts();
        gfx.set_render_scale(0.7);
        gfx.set_render_scale(1.0);
        assert_eq!(gfx.target_rebuilds, builds);
        assert_eq!(resource_creation_counts(), creations);
        assert!(gfx.history_valid);
    }

    #[test]
    fn settings_transaction_rebuilds_targets_and_invalidates_history() {
        let gpu = match platform::Gpu::new(wgpu::Instance::default(), None) {
            Ok(gpu) => gpu,
            Err(error) => {
                eprintln!("skipping settings-transaction GPU check: {error}");
                return;
            }
        };
        let mut settings = RenderTier::Medium.settings();
        settings.bloom = false;
        let created = Gfx::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, (128, 128), settings);
        assert!(created.is_ok(), "renderer failed to create test targets");
        let Ok(mut gfx) = created else {
            return;
        };
        let requested_msaa = gfx.msaa();
        let builds = gfx.target_rebuilds;
        gfx.history_valid = true;

        let mut next = gfx.settings();
        next.taa = true;
        let updated = gfx.set_settings(next);
        assert!(updated.is_ok(), "TAA settings update must build a plan");
        assert_eq!(gfx.target_rebuilds, builds.saturating_add(1));
        assert!(!gfx.history_valid);
        assert!(gfx.frame_plan().contains(FramePass::Taa));
        assert_eq!(gfx.msaa(), 1);

        let updated = gfx.set_view_mode(ViewMode::Lit);
        assert!(
            updated.is_ok(),
            "lit view settings update must build a plan"
        );
        let updated = gfx.set_taa(false);
        assert!(
            updated.is_ok(),
            "disabling TAA settings update must build a plan"
        );
        assert_eq!(gfx.msaa(), requested_msaa);
        assert!(!gfx.frame_plan().contains(FramePass::Taa));
    }

    #[test]
    fn composite_never_samples_sentinel_pixels_outside_scaled_viewport() {
        let gpu = match platform::Gpu::new(wgpu::Instance::default(), None) {
            Ok(gpu) => gpu,
            Err(error) => {
                eprintln!("skipping viewport sentinel GPU check: {error}");
                return;
            }
        };
        const SIZE: u32 = 128;
        const SCALE: f32 = 0.7;
        let mut settings = RenderTier::High.settings();
        settings.taa = false;
        settings.bloom = false;
        let created = Gfx::new(
            &gpu,
            wgpu::TextureFormat::Rgba8Unorm,
            (SIZE, SIZE),
            settings,
        );
        assert!(created.is_ok(), "renderer failed to create test targets");
        let Ok(mut gfx) = created else {
            return;
        };
        gfx.set_render_scale(SCALE);

        let active = crate::scale_px_ceil(SIZE, SCALE);
        let mut source = Vec::with_capacity((SIZE * SIZE * 8) as usize);
        for y in 0..SIZE {
            for x in 0..SIZE {
                if x < active && y < active {
                    source.extend_from_slice(&[0, 0, 0, 0x3c, 0, 0, 0, 0x3c]);
                } else {
                    source.extend_from_slice(&[0, 0x50, 0, 0, 0, 0x50, 0, 0x3c]);
                }
            }
        }
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gfx.targets.hdr_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &source,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 8),
                rows_per_image: Some(SIZE),
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        let post = PostU {
            a: [1.0, 0.0, 1.0, 0.0],
            b: [0.0; 4],
            c: [1.0, 1.0, 0.0, 0.0],
            shadows: [0.0; 4],
            highlights: [0.0; 4],
            flash_color: [1.0; 4],
            d: [0.0; 4],
            e: [SCALE, SCALE, SCALE, SCALE],
        };
        gpu.queue
            .write_buffer(&gfx.post_buf, 0, bytemuck::bytes_of(&post));

        let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport-sentinel-output"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ScreenSpace::fullscreen(
            &mut encoder,
            "viewport-sentinel-composite",
            &gfx.p_comp,
            &gfx.targets.post.zero.comp,
            &output_view,
            FullscreenOptions {
                clear: wgpu::Color::BLACK,
                viewport: None,
            },
            None,
        );
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport-sentinel-readback"),
            size: u64::from(SIZE) * u64::from(SIZE) * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE * 4),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        assert!(gpu.device.poll(wgpu::PollType::wait_indefinitely()).is_ok());
        assert!(receiver.recv().is_ok_and(|result| result.is_ok()));
        let pixels = readback.slice(..).get_mapped_range();
        assert!(pixels.is_ok());
        if let Ok(pixels) = pixels {
            assert!(!pixels.as_chunks::<4>().0.iter().any(|pixel| {
                pixel.first().copied().unwrap_or(0) > 220
                    && pixel.get(1).copied().unwrap_or(255) < 25
                    && pixel.get(2).copied().unwrap_or(0) > 220
            }));
        }
    }
}
