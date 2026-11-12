//! CPU submission, GPU uploads, uniforms, and per-frame encoder state.

use super::{Gfx, Globals, Light, Mat4, Particle, PostU, PreparedFrame, far_cascade_needs_redraw};
use crate::font::ATLAS;
use crate::ui::MAX_UI_VERTS;
use crate::{FrameStats, MAX_LIGHTS, MAX_PARTICLES, to_u32};
use bytemuck::Zeroable;

impl Gfx {
    /// Updates all CPU and GPU state shared by the checked pass encoders.
    pub(super) fn prepare_frame(&mut self, dt: f32) -> PreparedFrame {
        let asset_upload_bytes = std::mem::take(&mut self.asset_upload_bytes_pending);
        self.ensure_scene_gpu_capacity();
        let dirty_slots = self.scene.dirty_count();
        let cull_started = std::time::Instant::now();
        self.submit_scene();
        let cull_time_ns = u64::try_from(cull_started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let upload_started = std::time::Instant::now();
        self.flatten_visible_indices();
        let instance_bytes_uploaded = self.upload_scene_changes();
        if !self.index_staging.is_empty() {
            self.queue.write_buffer(
                &self.index_buf,
                0,
                bytemuck::cast_slice(&self.index_staging),
            );
        }
        let upload_time_ns = u64::try_from(upload_started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.time += dt;
        let cam = self.cam;
        let aspect = self.size.0 as f32 / self.size.1.max(1) as f32;
        let (cascades, texel) = self.cascades();
        let active_size = self.internal_size();
        let view = cam.view();
        let env = self.env;
        let mut lights = [Light::zeroed(); MAX_LIGHTS];
        for (destination, source) in lights.iter_mut().zip(&self.lights) {
            *destination = *source;
        }
        let globals = Globals {
            view_proj: self.jit_view_proj.to_cols_array_2d(),
            inv_view_proj: self.view_proj.inverse().to_cols_array_2d(),
            view: view.to_cols_array_2d(),
            vm_proj: Mat4::perspective_rh(cam.vm_fov_y, aspect, 0.01, 10.0).to_cols_array_2d(),
            cam_world: view.inverse().to_cols_array_2d(),
            cascade: [
                cascades[0].to_cols_array_2d(),
                cascades[1].to_cols_array_2d(),
            ],
            cam_pos: [cam.pos.x, cam.pos.y, cam.pos.z, self.time],
            sun_dir: env.sun_dir.normalize().extend(env.sun_intensity).to_array(),
            sun_color: [env.sun_color[0], env.sun_color[1], env.sun_color[2], 1.0],
            sky_zenith: [env.sky_zenith[0], env.sky_zenith[1], env.sky_zenith[2], 1.0],
            sky_horizon: [
                env.sky_horizon[0],
                env.sky_horizon[1],
                env.sky_horizon[2],
                1.0,
            ],
            ground: [env.ground[0], env.ground[1], env.ground[2], env.ambient],
            fog: [
                env.fog_density,
                env.fog_falloff,
                env.fog_base,
                env.fog_inscatter,
            ],
            fog_color: [env.fog_color[0], env.fog_color[1], env.fog_color[2], 1.0],
            shadow: [
                1.0 / self.settings.shadow_size as f32,
                self.settings.splits[0],
                self.settings.splits[1],
                if self.settings.pcf { 1.0 } else { 0.0 },
            ],
            misc: [self.lights.len() as f32, env.clouds, texel[0], texel[1]],
            tiles: [
                self.tiles.dims().0,
                self.tiles.dims().1,
                crate::tiles::TILE,
                0,
            ],
            lights,
            unjit_view_proj: self.view_proj.to_cols_array_2d(),
            prev_view_proj: self.prev_view_proj.to_cols_array_2d(),
            taa: [
                if self.history_valid { 1.0 } else { 0.0 },
                self.settings.render_scale,
                self.history_scale,
                0.0,
            ],
        };
        self.queue
            .write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));
        self.queue.write_buffer(
            &self.shadow_u[0],
            0,
            bytemuck::bytes_of(&cascades[0].to_cols_array_2d()),
        );
        self.queue.write_buffer(
            &self.shadow_u[1],
            0,
            bytemuck::bytes_of(&cascades[1].to_cols_array_2d()),
        );
        let post = self.post;
        let post_uniform = PostU {
            a: [
                post.exposure,
                if self.settings.bloom { post.bloom } else { 0.0 },
                post.bloom_threshold,
                post.vignette,
            ],
            b: [post.grain, self.time, post.damage, post.flash],
            c: [
                post.saturation,
                post.contrast,
                if self.format.is_srgb() { 0.0 } else { 1.0 },
                post.desaturate,
            ],
            shadows: post.shadow_tint,
            highlights: post.highlight_tint,
            flash_color: [
                post.flash_color[0],
                post.flash_color[1],
                post.flash_color[2],
                1.0,
            ],
            d: [
                if self.plan.has_auto_exposure() {
                    1.0
                } else {
                    0.0
                },
                0.0,
                0.18,
                0.0,
            ],
            e: [self.settings.render_scale; 4],
        };
        self.queue
            .write_buffer(&self.post_buf, 0, bytemuck::bytes_of(&post_uniform));

        let far_instances = self.shadow_staging.get(1).map(Vec::as_slice).unwrap_or(&[]);
        let far_bytes = bytemuck::cast_slice::<u32, u8>(far_instances);
        let cached_bytes = bytemuck::cast_slice::<u32, u8>(&self.far_shadow_cache);
        let far_matrix = self.cascade_mats.get(1).copied().unwrap_or(Mat4::IDENTITY);
        let moved_caster = far_instances
            .iter()
            .any(|slot| self.scene.dirty_slots().contains(slot));
        self.far_shadow_dirty = moved_caster
            || far_cascade_needs_redraw(
                self.far_shadow_valid,
                far_matrix,
                self.far_shadow_matrix,
                far_bytes,
                cached_bytes,
            );
        if self.far_shadow_dirty {
            self.far_shadow_cache.clear();
            self.far_shadow_cache.extend_from_slice(far_instances);
            self.far_shadow_matrix = far_matrix;
            self.far_shadow_valid = true;
        }
        let mut stats = FrameStats {
            culled: self.culled,
            dirty_slots: to_u32(dirty_slots),
            visible_main: self.visible_main,
            visible_shadow: self.visible_shadow,
            instance_bytes_uploaded,
            cull_time_ns,
            upload_time_ns,
            texture_bytes_by_family: self.texture_bytes_by_family(),
            asset_upload_bytes,
            ..Default::default()
        };
        let alpha_particles = self.particles[0].len();
        let additive_particles = self.particles[1].len();
        if alpha_particles > 0 {
            self.queue
                .write_buffer(&self.part_buf, 0, bytemuck::cast_slice(&self.particles[0]));
        }
        if additive_particles > 0 {
            let offset = (MAX_PARTICLES / 2 * std::mem::size_of::<Particle>()) as u64;
            self.queue.write_buffer(
                &self.part_buf,
                offset,
                bytemuck::cast_slice(&self.particles[1]),
            );
        }
        stats.particles = to_u32(alpha_particles + additive_particles);
        if let Some((low_row, high_row)) = self.ui.atlas.dirty.take()
            && let Some(pixels) = self
                .ui
                .atlas
                .pixels
                .get((low_row * ATLAS) as usize..(high_row * ATLAS) as usize)
        {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: low_row,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ATLAS),
                    rows_per_image: Some(high_row - low_row),
                },
                wgpu::Extent3d {
                    width: ATLAS,
                    height: high_row - low_row,
                    depth_or_array_layers: 1,
                },
            );
        }
        let ui_vertices = self.ui.verts.len().min(MAX_UI_VERTS);
        if let Some(vertices) = self
            .ui
            .verts
            .get(..ui_vertices)
            .filter(|vertices| !vertices.is_empty())
        {
            self.queue
                .write_buffer(&self.ui_buf, 0, bytemuck::cast_slice(vertices));
        }

        PreparedFrame {
            active_size,
            inv_view_proj: Mat4::from_cols_array_2d(&globals.inv_view_proj),
            alpha_particles,
            additive_particles,
            ui_vertices,
            prepass_scheduled: false,
            scene_available: false,
            post_slot: false,
            stats,
        }
    }
}
