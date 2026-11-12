//! Canonical WGSL sources and shader module creation.

use crate::RenderError;
use std::fmt;
#[cfg(feature = "hot-reload")]
use std::path::PathBuf;

/// Built-in WGSL module id (used in [`crate::RenderError::ShaderCompile`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderId {
    /// Main forward scene pass (concatenated multi-file module).
    Scene,
    /// Sun shadow depth pass.
    Shadow,
    /// Bloom, tonemap, and fullscreen post passes.
    Post,
    /// Screen-space GTAO generation.
    Gtao,
    /// GTAO spatial denoise.
    GtaoDenoise,
    /// HUD and text overlay.
    Ui,
    /// Per-tile light culling compute shader.
    Tiles,
    /// Temporal anti-aliasing resolve (history, neighbourhood clip, reprojection).
    Taa,
}

impl ShaderId {
    /// Every shader module the renderer builds.
    pub(crate) const ALL: [ShaderId; 8] = [
        ShaderId::Scene,
        ShaderId::Shadow,
        ShaderId::Post,
        ShaderId::Gtao,
        ShaderId::GtaoDenoise,
        ShaderId::Ui,
        ShaderId::Tiles,
        ShaderId::Taa,
    ];

    /// WGSL file names that make up this module (concatenated in order for `Scene`).
    pub(crate) fn files(self) -> &'static [&'static str] {
        match self {
            ShaderId::Scene => &["scene.wgsl", "scene_patterns.wgsl", "scene_lighting.wgsl"],
            ShaderId::Shadow => &["shadow.wgsl"],
            ShaderId::Post => &["post.wgsl"],
            ShaderId::Gtao => &["gtao.wgsl"],
            ShaderId::GtaoDenoise => &["gtao_denoise.wgsl"],
            ShaderId::Ui => &["ui.wgsl"],
            ShaderId::Tiles => &["tiles.wgsl"],
            ShaderId::Taa => &["taa.wgsl"],
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            ShaderId::Scene => "scene",
            ShaderId::Shadow => "shadow",
            ShaderId::Post => "post",
            ShaderId::Gtao => "gtao",
            ShaderId::GtaoDenoise => "gtao-denoise",
            ShaderId::Ui => "ui",
            ShaderId::Tiles => "tiles",
            ShaderId::Taa => "taa",
        }
    }
}

impl fmt::Display for ShaderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Supplies WGSL source for each [`ShaderId`].
#[derive(Clone, Copy)]
pub(crate) struct ShaderLibrary;

impl ShaderLibrary {
    pub(crate) fn new() -> Self {
        Self
    }

    /// Full WGSL source for `id`.
    pub(crate) fn source(&self, id: ShaderId) -> Result<String, RenderError> {
        #[cfg(feature = "hot-reload")]
        {
            self.read_disk(id)
        }
        #[cfg(not(feature = "hot-reload"))]
        {
            Ok(embedded_source(id))
        }
    }
}

impl ShaderLibrary {
    #[cfg(feature = "hot-reload")]
    fn read_disk(&self, id: ShaderId) -> Result<String, RenderError> {
        let root = shader_dir();
        let mut out = String::new();
        for (i, name) in id.files().iter().enumerate() {
            let path = root.join(name);
            let text =
                std::fs::read_to_string(&path).map_err(|source| RenderError::ShaderRead {
                    path: path.clone(),
                    source,
                })?;
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&text);
        }
        Ok(out)
    }
}

#[cfg(feature = "hot-reload")]
pub(crate) fn shader_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src/shaders/"))
}

#[cfg(not(feature = "hot-reload"))]
fn embedded_source(id: ShaderId) -> String {
    match id {
        ShaderId::Scene => format!(
            "{}\n{}\n{}",
            include_str!("shaders/scene.wgsl"),
            include_str!("shaders/scene_patterns.wgsl"),
            include_str!("shaders/scene_lighting.wgsl"),
        ),
        ShaderId::Shadow => include_str!("shaders/shadow.wgsl").to_owned(),
        ShaderId::Post => include_str!("shaders/post.wgsl").to_owned(),
        ShaderId::Gtao => include_str!("shaders/gtao.wgsl").to_owned(),
        ShaderId::GtaoDenoise => include_str!("shaders/gtao_denoise.wgsl").to_owned(),
        ShaderId::Ui => include_str!("shaders/ui.wgsl").to_owned(),
        ShaderId::Tiles => include_str!("shaders/tiles.wgsl").to_owned(),
        ShaderId::Taa => include_str!("shaders/taa.wgsl").to_owned(),
    }
}

/// Creates a shader module, surfacing validation errors as [`RenderError::ShaderCompile`].
pub(crate) fn create_module(
    device: &wgpu::Device,
    id: ShaderId,
    library: &ShaderLibrary,
) -> Result<wgpu::ShaderModule, RenderError> {
    create_module_wgsl(device, id, library.source(id)?)
}

fn create_module_wgsl(
    device: &wgpu::Device,
    id: ShaderId,
    source: String,
) -> Result<wgpu::ShaderModule, RenderError> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(id.label()),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let validation = pollster::block_on(scope.pop());
    if let Some(err) = validation {
        return Err(RenderError::ShaderCompile {
            id,
            message: err.to_string(),
        });
    }
    Ok(module)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn shader_ids_cover_shader_dir_exactly_once() {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/shaders"));
        let mut on_disk = HashSet::new();
        let entries = std::fs::read_dir(dir).unwrap();
        for ent in entries {
            let Ok(ent) = ent else { continue };
            if ent.path().extension().is_some_and(|e| e == "wgsl") {
                let name = ent.file_name().to_string_lossy().into_owned();
                assert!(on_disk.insert(name), "duplicate shader file");
            }
        }
        let mut from_ids = HashSet::<String>::new();
        for id in ShaderId::ALL {
            for f in id.files() {
                assert!(
                    from_ids.insert((*f).to_owned()),
                    "duplicate mapping for {f}"
                );
            }
        }
        assert_eq!(from_ids, on_disk, "ShaderId::ALL must match shader dir");
        assert_eq!(
            ShaderId::Scene.files(),
            &["scene.wgsl", "scene_patterns.wgsl", "scene_lighting.wgsl",],
        );
    }

    #[test]
    fn broken_wgsl_returns_shader_compile() {
        let instance = wgpu::Instance::default();
        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            })) {
                Ok(a) => a,
                Err(_) => {
                    println!("skip broken_wgsl_returns_shader_compile: no GPU adapter");
                    return;
                }
            };
        let Ok((device, _queue)) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("shader-test"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            }))
        else {
            println!("skip broken_wgsl_returns_shader_compile: request_device failed");
            return;
        };
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let _ = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("broken"),
            source: wgpu::ShaderSource::Wgsl("not valid wgsl !!!".into()),
        });
        let validation = pollster::block_on(scope.pop());
        if validation.is_none() {
            println!("skip broken_wgsl_returns_shader_compile: adapter did not report validation");
            return;
        }
        let err = create_module_wgsl(&device, ShaderId::Post, "@".into());
        assert!(matches!(err, Err(RenderError::ShaderCompile { .. })));
    }
}
