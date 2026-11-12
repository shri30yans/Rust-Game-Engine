//! Hot shader reload for [`super::Gfx`].

use super::pipelines::scene::ScenePipelines;
use super::pipelines::{self, PostPipelines};
use super::*;
use crate::shaders::{ShaderId, create_module};

impl Gfx {
    /// Rebuilds the module and pipelines for `id`, keeping the previous GPU state on failure.
    pub(crate) fn rebuild(&mut self, id: ShaderId) -> Result<(), RenderError> {
        let device = &self.device;
        let library = &self.shader_library;
        match id {
            ShaderId::Scene => {
                let sm = create_module(device, id, library)?;
                let scene =
                    pipelines::create_scene_pipelines(device, &self.layouts, &sm, &self.pipe_ctx);
                apply_scene(
                    &mut self.p_scene,
                    &mut self.p_base_colour,
                    &mut self.p_lighting_only,
                    &mut self.p_vm,
                    &mut self.p_base_colour_vm,
                    &mut self.p_lighting_only_vm,
                    &mut self.p_coverage,
                    &mut self.p_coverage_vm,
                    &mut self.p_sky,
                    &mut self.p_glow,
                    &mut self.p_part_alpha,
                    &mut self.p_part_add,
                    scene,
                );
                self.scene_sm = sm;
                if let Some(ss) = self.screen.as_mut() {
                    ss.rebuild_scene(device, &self.scene_sm);
                }
            }
            ShaderId::Shadow => {
                let sm = create_module(device, id, library)?;
                let p = pipelines::create_shadow_pipeline(device, &self.layouts, &sm);
                self.shadow_sm = sm;
                self.p_shadow = p;
            }
            ShaderId::Post => {
                let sm = create_module(device, id, library)?;
                let post =
                    pipelines::create_post_pipelines(device, &self.layouts, &sm, &self.pipe_ctx);
                let debug =
                    pipelines::create_debug_pipelines(device, &self.layouts, &sm, &self.pipe_ctx);
                apply_post(
                    &mut self.p_down_first,
                    &mut self.p_down,
                    &mut self.p_up,
                    &mut self.p_comp,
                    &mut self.p_debug_hdr,
                    &mut self.p_debug_ao,
                    &mut self.p_debug_depth,
                    &mut self.p_debug_coverage,
                    &mut self.p_debug_shadow,
                    post,
                    debug,
                );
                self.post_sm = sm;
                if let Some(ss) = self.screen.as_mut() {
                    ss.rebuild_post(device, &self.post_sm);
                }
            }
            ShaderId::Gtao => {
                let sm = create_module(device, id, library)?;
                if let Some(ss) = self.screen.as_mut() {
                    ss.rebuild_gtao(device, &sm);
                }
                self.gtao_sm = sm;
            }
            ShaderId::GtaoDenoise => {
                let sm = create_module(device, id, library)?;
                if let Some(ss) = self.screen.as_mut() {
                    ss.rebuild_denoise(device, &sm);
                }
                self.denoise_sm = sm;
            }
            ShaderId::Ui => {
                let sm = create_module(device, id, library)?;
                let p = pipelines::create_ui_pipeline(device, &self.layouts, &sm, &self.pipe_ctx);
                self.ui_sm = sm;
                self.p_ui = p;
            }
            ShaderId::Tiles => {
                let sm = self.tiles.rebuild_shader(device, library)?;
                self.tiles_sm = sm;
            }
            ShaderId::Taa => {
                let sm = create_module(device, id, library)?;
                if let Some(ss) = self.screen.as_mut() {
                    ss.rebuild_taa(device, &sm);
                }
                self.taa_sm = sm;
            }
        }
        Ok(())
    }

    #[cfg(feature = "hot-reload")]
    pub(crate) fn poll_shader_hot_reload(&mut self, now: std::time::Instant) {
        let changed = match self.shader_watcher.poll(now) {
            Ok(list) => list,
            Err(e) => {
                eprintln!("shader hot-reload: {e}");
                self.hot_reload_clock = now;
                return;
            }
        };
        for id in changed.iter() {
            match self.rebuild(id) {
                Ok(()) => eprintln!("shader hot-reload: rebuilt {id}"),
                Err(e) => eprintln!("shader hot-reload: {e}"),
            }
        }
        self.hot_reload_clock = now;
    }
}

#[allow(clippy::too_many_arguments)] // Hot reload replaces the complete scene pipeline bundle atomically.
fn apply_scene(
    p_scene: &mut [[wgpu::RenderPipeline; 3]; 2],
    p_base_colour: &mut [[wgpu::RenderPipeline; 3]; 2],
    p_lighting_only: &mut [[wgpu::RenderPipeline; 3]; 2],
    p_vm: &mut [[wgpu::RenderPipeline; 3]; 2],
    p_base_colour_vm: &mut [[wgpu::RenderPipeline; 3]; 2],
    p_lighting_only_vm: &mut [[wgpu::RenderPipeline; 3]; 2],
    p_coverage: &mut [[wgpu::RenderPipeline; 3]; 2],
    p_coverage_vm: &mut [[wgpu::RenderPipeline; 3]; 2],
    p_sky: &mut wgpu::RenderPipeline,
    p_glow: &mut [[wgpu::RenderPipeline; 3]; 2],
    p_part_alpha: &mut wgpu::RenderPipeline,
    p_part_add: &mut wgpu::RenderPipeline,
    scene: ScenePipelines,
) {
    *p_scene = scene.p_scene;
    *p_base_colour = scene.p_base_colour;
    *p_lighting_only = scene.p_lighting_only;
    *p_vm = scene.p_vm;
    *p_base_colour_vm = scene.p_base_colour_vm;
    *p_lighting_only_vm = scene.p_lighting_only_vm;
    *p_coverage = scene.p_coverage;
    *p_coverage_vm = scene.p_coverage_vm;
    *p_sky = scene.p_sky;
    *p_glow = scene.p_glow;
    *p_part_alpha = scene.p_part_alpha;
    *p_part_add = scene.p_part_add;
}

#[allow(clippy::too_many_arguments)] // Hot reload replaces the complete post and debug pipeline bundle atomically.
fn apply_post(
    p_down_first: &mut wgpu::RenderPipeline,
    p_down: &mut wgpu::RenderPipeline,
    p_up: &mut wgpu::RenderPipeline,
    p_comp: &mut wgpu::RenderPipeline,
    p_debug_hdr: &mut wgpu::RenderPipeline,
    p_debug_ao: &mut wgpu::RenderPipeline,
    p_debug_depth: &mut wgpu::RenderPipeline,
    p_debug_coverage: &mut wgpu::RenderPipeline,
    p_debug_shadow: &mut wgpu::RenderPipeline,
    post: PostPipelines,
    debug: pipelines::DebugPipelines,
) {
    *p_down_first = post.p_down_first;
    *p_down = post.p_down;
    *p_up = post.p_up;
    *p_comp = post.p_comp;
    *p_debug_hdr = debug.p_hdr;
    *p_debug_ao = debug.p_ao;
    *p_debug_depth = debug.p_depth;
    *p_debug_coverage = debug.p_coverage;
    *p_debug_shadow = debug.p_shadow;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::Settings;
    use super::*;
    use crate::ShaderId;
    use platform::Gpu;

    #[test]
    fn rebuild_post_succeeds_on_embedded_sources() {
        let instance = wgpu::Instance::default();
        let gpu = match Gpu::new(instance, None) {
            Ok(g) => g,
            Err(e) => {
                println!("skip rebuild_post_succeeds_on_embedded_sources: {e}");
                return;
            }
        };
        let mut gfx = match Gfx::new(
            &gpu,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            (640, 360),
            Settings::default(),
        ) {
            Ok(g) => g,
            Err(e) => {
                println!("skip rebuild_post_succeeds_on_embedded_sources: Gfx::new: {e}");
                return;
            }
        };
        assert!(
            gfx.rebuild(ShaderId::Post).is_ok(),
            "rebuild ShaderId::Post should succeed on shipped WGSL"
        );
    }
}
