//! Tiled-light culling and HDR scene pass encoding.

use super::{FrameStats, Gfx, GpuPass, MAX_PARTICLES, Particle, to_u32};

impl Gfx {
    /// Encodes light culling and all opaque, sky, emissive, and particle scene drawing.
    ///
    /// Returns false only when a defensive missing-depth fallback prevents a scene pass.
    pub(super) fn encode_scene(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        active_size: (u32, u32),
        prepass_scheduled: bool,
        view_mode: crate::ViewMode,
        particle_counts: (usize, usize),
        stats: &mut FrameStats,
    ) -> bool {
        let (alpha_particles, additive_particles) = particle_counts;
        self.tiles.cull(encoder, &self.globals_bg);
        let depth_load = if prepass_scheduled {
            wgpu::LoadOp::Load
        } else {
            wgpu::LoadOp::Clear(1.0)
        };
        let Some(depth_view) = self.depth_view(&self.targets) else {
            return false;
        };
        let targets = &self.targets;
        let (colour_view, resolve_target) = match &targets.msaa {
            Some(multisample) => (multisample, Some(&targets.hdr)),
            None => (&targets.hdr, None),
        };
        let timestamps = self
            .timer
            .as_mut()
            .and_then(|timer| timer.writes(GpuPass::Scene));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: colour_view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: if resolve_target.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: depth_load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(
            0.0,
            0.0,
            active_size.0 as f32,
            active_size.1 as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(0, 0, active_size.0, active_size.1);
        pass.set_bind_group(0, &self.view_bg, &[]);
        let (scene_pipelines, viewmodel_pipelines) = match view_mode {
            crate::ViewMode::BaseColour => (&self.p_base_colour, &self.p_base_colour_vm),
            crate::ViewMode::LightingOnly => (&self.p_lighting_only, &self.p_lighting_only_vm),
            crate::ViewMode::OpaqueCoverage { .. } => (&self.p_coverage, &self.p_coverage_vm),
            crate::ViewMode::Lit
            | crate::ViewMode::AmbientOcclusion
            | crate::ViewMode::Depth
            | crate::ViewMode::Shadow => (&self.p_scene, &self.p_vm),
        };
        for bucket in &self.buckets {
            let Some(mesh) = self.meshes.get(bucket.mesh.0) else {
                continue;
            };
            let Some(lod) = mesh.lod(bucket.lod) else {
                continue;
            };
            let ranges = &bucket.ranges;
            let count = ranges.both + ranges.mo;
            if count == 0 {
                continue;
            }
            let material_kind = usize::from(bucket.material.is_some());
            let Some(pipeline) = scene_pipelines
                .get(material_kind)
                .and_then(|set| set.get(lod.format.pipeline_index()))
            else {
                continue;
            };
            if let Some(id) = bucket.material
                && let Some(material) = self.materials.get(id.0)
            {
                pass.set_bind_group(1, &material.bind_group, &[]);
            }
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, lod.vb.slice(..));
            pass.set_vertex_buffer(1, lod.attribute_vb.slice(..));
            pass.set_index_buffer(lod.ib.slice(..), lod.index_format);
            pass.draw_indexed(
                lod.indices.clone(),
                lod.vertex_base,
                ranges.start..ranges.start.saturating_add(count),
            );
            stats.draws += 1;
            stats.instances += count;
            stats.tris +=
                (lod.indices.end.saturating_sub(lod.indices.start) / 3) as u64 * count as u64;
            if let Some(lod_count) = stats.lod_counts.get_mut(bucket.lod) {
                *lod_count = lod_count.saturating_add(count);
            }
        }
        for bucket in &self.buckets {
            let Some(mesh) = self.meshes.get(bucket.mesh.0) else {
                continue;
            };
            let Some(lod) = mesh.lod(bucket.lod) else {
                continue;
            };
            let ranges = &bucket.ranges;
            if ranges.vm == 0 {
                continue;
            }
            let material_kind = usize::from(bucket.material.is_some());
            let Some(pipeline) = viewmodel_pipelines
                .get(material_kind)
                .and_then(|set| set.get(lod.format.pipeline_index()))
            else {
                continue;
            };
            if let Some(id) = bucket.material
                && let Some(material) = self.materials.get(id.0)
            {
                pass.set_bind_group(1, &material.bind_group, &[]);
            }
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, lod.vb.slice(..));
            pass.set_vertex_buffer(1, lod.attribute_vb.slice(..));
            pass.set_index_buffer(lod.ib.slice(..), lod.index_format);
            pass.draw_indexed(
                lod.indices.clone(),
                lod.vertex_base,
                ranges.vm_start..ranges.vm_start.saturating_add(ranges.vm),
            );
            stats.draws += 1;
            stats.instances += ranges.vm;
            stats.tris +=
                (lod.indices.end.saturating_sub(lod.indices.start) / 3) as u64 * ranges.vm as u64;
            if let Some(lod_count) = stats.lod_counts.get_mut(bucket.lod) {
                *lod_count = lod_count.saturating_add(ranges.vm);
            }
        }
        if matches!(view_mode, crate::ViewMode::OpaqueCoverage { .. }) {
            return true;
        }
        pass.set_pipeline(&self.p_sky);
        pass.draw(0..3, 0..1);
        stats.draws += 1;
        for bucket in &self.buckets {
            let Some(mesh) = self.meshes.get(bucket.mesh.0) else {
                continue;
            };
            let Some(lod) = mesh.lod(bucket.lod) else {
                continue;
            };
            let ranges = &bucket.ranges;
            if ranges.glow == 0 {
                continue;
            }
            let material_kind = usize::from(bucket.material.is_some());
            let Some(pipeline) = self
                .p_glow
                .get(material_kind)
                .and_then(|set| set.get(lod.format.pipeline_index()))
            else {
                continue;
            };
            if let Some(id) = bucket.material
                && let Some(material) = self.materials.get(id.0)
            {
                pass.set_bind_group(1, &material.bind_group, &[]);
            }
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, lod.vb.slice(..));
            pass.set_vertex_buffer(1, lod.attribute_vb.slice(..));
            pass.set_index_buffer(lod.ib.slice(..), lod.index_format);
            pass.draw_indexed(
                lod.indices.clone(),
                lod.vertex_base,
                ranges.glow_start..ranges.glow_start.saturating_add(ranges.glow),
            );
            stats.draws += 1;
            if let Some(lod_count) = stats.lod_counts.get_mut(bucket.lod) {
                *lod_count = lod_count.saturating_add(ranges.glow);
            }
        }
        let particle_size = std::mem::size_of::<Particle>() as u64;
        if additive_particles > 0 {
            pass.set_pipeline(&self.p_part_add);
            pass.set_vertex_buffer(
                0,
                self.part_buf
                    .slice((MAX_PARTICLES / 2) as u64 * particle_size..),
            );
            pass.draw(0..6, 0..to_u32(additive_particles));
            stats.draws += 1;
        }
        if alpha_particles > 0 {
            pass.set_pipeline(&self.p_part_alpha);
            pass.set_vertex_buffer(0, self.part_buf.slice(..));
            pass.draw(0..6, 0..to_u32(alpha_particles));
            stats.draws += 1;
        }
        true
    }
}
