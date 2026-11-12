//! Headless stress bench: a dense city-block scene at a fixed resolution. Prints one JSON line
//! with wall frame time and per-stage GPU time so regressions and wins are measurable.
//!
//! `cargo run --release -p engine-render --example bench -- [--size 1920x1080] [--grid 40] [--msaa 4]
//!  [--lights 32] [--frames 240] [--taa on|off] [--shot out.png]`
use engine_assets::{TextureData, prepare_texture};
use glam::{Mat4, Quat, Vec3};
use platform::{Gpu, Input, PlatformApp};
use render::{GPU_PASSES, *};
use std::path::PathBuf;

struct Opts {
    size: (u32, u32),
    grid: i32,
    msaa: u32,
    lights: u32,
    frames: u32,
    /// TAA on the High tier (default on); `--taa off` measures the frame without it.
    taa: bool,
    /// Use a shared UASTC texture material for the grid scene.
    textured: bool,
    shot: Option<PathBuf>,
}

fn parse() -> Opts {
    let mut o = Opts {
        size: (1920, 1080),
        grid: 40,
        msaa: 4,
        lights: 32,
        frames: 240,
        taa: true,
        textured: false,
        shot: None,
    };
    let mut a = std::env::args().skip(1);
    while let Some(k) = a.next() {
        let v = a.next().unwrap_or_default();
        match k.as_str() {
            "--size" => {
                if let Some((w, h)) = v.split_once('x') {
                    o.size = (w.parse().unwrap_or(1920), h.parse().unwrap_or(1080));
                }
            }
            "--grid" => o.grid = v.parse().unwrap_or(40),
            "--msaa" => o.msaa = v.parse().unwrap_or(4),
            "--lights" => o.lights = v.parse().unwrap_or(32),
            "--frames" => o.frames = v.parse().unwrap_or(240),
            "--taa" => o.taa = v != "off",
            "--textured" => o.textured = v == "on",
            "--shot" => o.shot = Some(PathBuf::from(v)),
            _ => {}
        }
    }
    o
}

struct Bench {
    o: Opts,
    g: Option<Gfx>,
    cube: MeshId,
    ball: MeshId,
    material: Option<MaterialId>,
    t: f32,
    gpu: Vec<GpuTimes>,
    cpu: Vec<FrameStats>,
    last: FrameStats,
}

impl PlatformApp for Bench {
    fn init(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, size: (u32, u32)) {
        let mut settings = RenderTier::High.settings();
        settings.msaa = self.o.msaa;
        settings.max_lights = self.o.lights;
        settings.taa = self.o.taa;
        let mut g = match Gfx::new(gpu, format, size, settings) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("bench: {e}");
                std::process::exit(1);
            }
        };
        self.cube = match g.add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0))) {
            Ok(mesh) => mesh,
            Err(error) => {
                eprintln!("bench: {error}");
                std::process::exit(1);
            }
        };
        let mut b = MeshBuilder::new();
        b.sphere(
            Vec3::ZERO,
            Vec3::splat(0.5),
            24,
            16,
            Mat::hex(0xffffff, 0.3, 0.0),
        );
        self.ball = match g.add_mesh(&b) {
            Ok(mesh) => mesh,
            Err(error) => {
                eprintln!("bench: {error}");
                std::process::exit(1);
            }
        };
        if self.o.textured {
            let data = TextureData {
                ktx2: include_bytes!("../../assets/tests/fixtures/uastc.ktx2").to_vec(),
            };
            let prepared = match prepare_texture(&data, ColourSpace::Srgb, g.texture_family()) {
                Ok(prepared) => prepared,
                Err(error) => {
                    eprintln!("bench texture: {error}");
                    std::process::exit(1);
                }
            };
            let texture = match g.add_texture(prepared) {
                Ok(texture) => texture,
                Err(error) => {
                    eprintln!("bench texture: {error}");
                    std::process::exit(1);
                }
            };
            self.material = match g.add_material(MaterialDesc {
                base_color_texture: Some(texture),
                ..MaterialDesc::default()
            }) {
                Ok(material) => Some(material),
                Err(error) => {
                    eprintln!("bench material: {error}");
                    std::process::exit(1);
                }
            };
        }
        let mut add = |mesh, matrix, look| {
            g.scene_mut().add(
                mesh,
                PrimitiveDesc {
                    transform: glam::Affine3A::from_mat4(matrix),
                    material: self
                        .material
                        .map_or(Material::Procedural(look), Material::Textured),
                    layer: Layer::World,
                    casts_shadow: true,
                },
            )
        };
        add(
            self.cube,
            Mat4::from_scale_rotation_translation(
                Vec3::new(400.0, 1.0, 400.0),
                Quat::IDENTITY,
                Vec3::new(0.0, -0.5, 0.0),
            ),
            Look {
                tint: [0.48, 0.50, 0.53, 1.0],
                pattern: Some(Pattern::Concrete),
                ..Look::default()
            },
        )
        .unwrap_or_else(|e| {
            eprintln!("bench scene: {e}");
            std::process::exit(1);
        });
        let pats = [
            Pattern::Brick,
            Pattern::Tiles,
            Pattern::Corrugated,
            Pattern::Wood,
            Pattern::Windows,
            Pattern::Hazard,
        ];
        let n = self.o.grid;
        for ix in -n..n {
            for iz in -n..n {
                let k = (ix * 31 + iz * 17).unsigned_abs();
                let h = 2.0 + (k % 7) as f32 * 1.5;
                let pos = Vec3::new(ix as f32 * 6.0, h * 0.5, -(iz as f32) * 6.0);
                let pat = pats.get((k % 6) as usize).copied().unwrap_or(Pattern::None);
                add(
                    self.cube,
                    Mat4::from_scale_rotation_translation(
                        Vec3::new(3.0, h, 3.0),
                        Quat::IDENTITY,
                        pos,
                    ),
                    Look {
                        tint: [0.50, 0.53, 0.56, 1.0],
                        pattern: Some(pat),
                        ..Look::default()
                    },
                )
                .unwrap_or_else(|e| {
                    eprintln!("bench scene: {e}");
                    std::process::exit(1);
                });
                if k % 5 == 0 {
                    add(
                        self.ball,
                        Mat4::from_translation(pos + Vec3::new(2.5, -h * 0.5 + 0.6, 0.0))
                            * Mat4::from_scale(Vec3::splat(1.2)),
                        Look {
                            tint: [0.784, 0.333, 0.239, 1.0],
                            metalness: Some(0.6),
                            roughness: Some(0.3),
                            ..Look::default()
                        },
                    )
                    .unwrap_or_else(|e| {
                        eprintln!("bench scene: {e}");
                        std::process::exit(1);
                    });
                }
            }
        }
        self.g = Some(g);
    }
    fn resize(&mut self, s: (u32, u32)) {
        if let Some(g) = self.g.as_mut() {
            g.resize(s);
        }
    }
    fn frame(&mut self, target: &wgpu::TextureView, _i: &mut Input, dt: f32) {
        let Some(g) = self.g.as_mut() else { return };
        self.t += dt;
        let cam = Camera {
            pos: Vec3::new(0.0, 6.0, 40.0),
            pitch: -0.1,
            yaw: (self.t * 0.2).sin() * 0.2,
            ..Default::default()
        };
        g.begin_frame(cam);
        for i in 0..self.o.lights {
            let a = i as f32 * 0.7 + self.t;
            g.light(
                Vec3::new(a.cos() * 20.0, 2.0, 30.0 - i as f32 * 3.0),
                10.0,
                [1.0, 0.6, 0.3],
                5.0,
            );
        }
        self.last = g.render(target, dt);
        self.cpu.push(self.last);
        if let Some(t) = g.gpu_times() {
            self.gpu.push(t);
        }
    }
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let o = parse();
    let (size, frames, shot) = (o.size, o.frames, o.shot.clone());
    let mut app = Bench {
        o,
        g: None,
        cube: MeshId::procedural(0),
        ball: MeshId::procedural(0),
        material: None,
        t: 0.0,
        gpu: Vec::new(),
        cpu: Vec::new(),
        last: FrameStats::default(),
    };
    let r = platform::headless::run(&mut app, size, frames, shot.as_deref())?;
    let skip = (frames as usize / 4).min(r.frame_ms.len());
    let wall: Vec<f32> = r.frame_ms.get(skip..).unwrap_or(&[]).to_vec();
    let g = app
        .gpu
        .get(app.gpu.len().saturating_sub(app.gpu.len() / 2)..)
        .unwrap_or(&[]);
    let m = |f: fn(&GpuTimes) -> f32| median(g.iter().map(f).collect());
    let s = app.last;
    let actual_msaa = app.g.as_ref().map(Gfx::msaa).unwrap_or(1);
    let cpu = app
        .cpu
        .get(app.cpu.len().saturating_sub(app.cpu.len() / 2)..)
        .unwrap_or(&[]);
    let cpu_median = |f: fn(&FrameStats) -> u64| {
        median(
            cpu.iter()
                .map(|sample| f(sample) as f32 / 1_000_000.0)
                .collect(),
        )
    };
    let pass_times = GPU_PASSES
        .iter()
        .map(|pass| {
            let name = format!("{pass:?}");
            let value = median(
                g.iter()
                    .map(|t| t.passes.get(*pass as usize).copied().unwrap_or(0.0))
                    .collect(),
            );
            format!("\"{name}\":{value:.4}")
        })
        .collect::<Vec<_>>()
        .join(",");
    let p50 = median(wall.clone());
    let p99 = {
        let mut w = wall;
        w.sort_by(f32::total_cmp);
        w.get((w.len() * 99 / 100).min(w.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0.0)
    };
    println!(
        "{{\"size\":\"{}x{}\",\"msaa\":{},\"wall_ms_p50\":{:.3},\"wall_ms_p99\":{:.3},\"gpu_samples\":{},\"gpu_ms\":{{\"frame\":{:.3},\"shadow\":{:.3},\"prepass\":{:.3},\"gtao\":{:.3},\"scene\":{:.3},\"bloom\":{:.3},\"composite\":{:.3},\"taa\":{:.3},\"passes_overlapping\":{{{}}}}},\"taa_on\":{},\"draws\":{},\"shadow_draws\":{},\"tris\":{},\"instances\":{},\"culled\":{},\"instance_bytes_uploaded\":{},\"dirty_slots\":{},\"cpu_ms\":{{\"cull\":{:.4},\"upload\":{:.4},\"encode\":{:.4}}}}}",
        size.0,
        size.1,
        actual_msaa,
        p50,
        p99,
        g.len(),
        m(|t| t.frame),
        m(|t| t.shadow),
        m(|t| t.prepass),
        m(|t| t.gtao),
        m(|t| t.scene),
        m(|t| t.bloom),
        m(|t| t.composite),
        m(|t| t.taa),
        pass_times,
        app.o.taa,
        s.draws,
        s.shadow_draws,
        s.tris,
        s.instances,
        s.culled,
        s.instance_bytes_uploaded,
        s.dirty_slots,
        cpu_median(|sample| sample.cull_time_ns),
        cpu_median(|sample| sample.upload_time_ns),
        cpu_median(|sample| sample.encode_time_ns)
    );
    Ok(())
}
