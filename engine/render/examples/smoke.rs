//! Headless smoke test: builds every pipeline, renders a lit scene with shadows,
//! lights, particles, glow and UI text, writes a PNG.
use glam::{Mat4, Quat, Vec3};
use platform::{Gpu, Input, PlatformApp};
use render::*;

struct App {
    g: Option<Gfx>,
    cube: MeshId,
    ball: MeshId,
    font: FontId,
}

impl PlatformApp for App {
    fn init(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, size: (u32, u32)) {
        let mut g = match Gfx::new(gpu, format, size, Settings::default()) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("smoke: {e}");
                std::process::exit(1);
            }
        };
        self.cube = match g.add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0))) {
            Ok(mesh) => mesh,
            Err(error) => {
                eprintln!("smoke: {error}");
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
                eprintln!("smoke: {error}");
                std::process::exit(1);
            }
        };
        let mut add = |mesh, matrix, look, layer, casts_shadow| {
            if let Err(error) = g.scene_mut().add(
                mesh,
                PrimitiveDesc {
                    transform: glam::Affine3A::from_mat4(matrix),
                    material: Material::Procedural(look),
                    layer,
                    casts_shadow,
                },
            ) {
                eprintln!("smoke scene: {error}");
                std::process::exit(1);
            }
        };
        add(
            self.cube,
            Mat4::from_scale_rotation_translation(
                Vec3::new(60.0, 1.0, 60.0),
                Quat::IDENTITY,
                Vec3::new(0.0, -0.5, 0.0),
            ),
            Look {
                tint: {
                    let rgb = srgb(0x8a8f96);
                    [rgb[0], rgb[1], rgb[2], 1.0]
                },
                pattern: Some(Pattern::Concrete),
                ..Look::default()
            },
            Layer::World,
            true,
        );
        let pats = [
            Pattern::Brick,
            Pattern::Tiles,
            Pattern::Corrugated,
            Pattern::Wood,
            Pattern::Windows,
            Pattern::Hazard,
        ];
        let cols = [0xc8553d, 0xe0e0e0, 0x3d7ec8, 0xa06a3a, 0x9aa7b5, 0xf2c230];
        for (i, (&pat, &col)) in pats.iter().zip(cols.iter()).enumerate() {
            let x = -7.5 + i as f32 * 3.0;
            let tint = {
                let rgb = srgb(col);
                [rgb[0], rgb[1], rgb[2], 1.0]
            };
            add(
                self.cube,
                Mat4::from_scale_rotation_translation(
                    Vec3::new(2.2, 3.0 + i as f32 * 0.6, 2.2),
                    Quat::from_rotation_y(0.3 * i as f32),
                    Vec3::new(x, 1.5 + i as f32 * 0.3, -3.0),
                ),
                Look {
                    tint,
                    pattern: Some(pat),
                    ..Look::default()
                },
                Layer::World,
                true,
            );
            add(
                self.ball,
                Mat4::from_translation(Vec3::new(x, 0.6, 2.0)) * Mat4::from_scale(Vec3::splat(1.2)),
                Look {
                    tint,
                    metalness: Some(i as f32 / 5.0),
                    roughness: Some(0.15 + 0.15 * i as f32),
                    ..Look::default()
                },
                Layer::World,
                true,
            );
        }
        add(
            self.ball,
            Mat4::from_translation(Vec3::new(0.0, 1.6, 5.0)) * Mat4::from_scale(Vec3::splat(0.4)),
            Look {
                tint: [1.0, 0.4, 0.1, 1.0],
                emissive: 8.0,
                ..Look::default()
            },
            Layer::World,
            false,
        );
        add(
            self.ball,
            Mat4::from_translation(Vec3::new(4.0, 2.0, 4.0)) * Mat4::from_scale(Vec3::splat(3.0)),
            Look {
                tint: [0.2, 0.6, 1.0, 1.0],
                emissive: 0.6,
                ..Look::default()
            },
            Layer::Glow,
            false,
        );
        self.font = match g
            .ui
            .atlas
            .add_font(include_bytes!("../fonts/Roboto-BlackItalic.ttf"))
        {
            Ok(id) => id,
            Err(e) => {
                eprintln!("smoke: {e}");
                std::process::exit(1);
            }
        };
        self.g = Some(g);
    }
    fn resize(&mut self, s: (u32, u32)) {
        if let Some(g) = self.g.as_mut() {
            g.resize(s);
        }
    }
    fn frame(&mut self, target: &wgpu::TextureView, _i: &mut Input, dt: f32) {
        let Some(g) = self.g.as_mut() else { return };
        let cam = Camera {
            pos: Vec3::new(0.0, 3.0, 12.0),
            pitch: -0.12,
            ..Default::default()
        };
        g.begin_frame(cam);
        g.light(Vec3::new(0.0, 1.6, 5.0), 8.0, [1.0, 0.5, 0.2], 6.0);
        for k in 0..60 {
            let a = k as f32 * 0.37;
            g.particle(
                Particle {
                    pos: [-4.0 + a.cos(), 1.0 + k as f32 * 0.05, 5.0 + a.sin()],
                    size: 0.12,
                    color: [1.0, 0.7, 0.3, 1.0],
                    vel: [0.0, 0.3, 0.0],
                    kind: 1.0,
                },
                true,
            );
            g.particle(
                Particle {
                    pos: [5.0 + a.cos() * 0.5, 0.5 + k as f32 * 0.06, 3.0],
                    size: 0.6,
                    color: [0.3, 0.3, 0.3, 0.5],
                    vel: [0.0; 3],
                    kind: 2.0,
                },
                false,
            );
        }
        g.ui.rect(30.0, 30.0, 320.0, 70.0, 10.0, rgba(0x101418, 0.7));
        g.ui.text(
            self.font,
            46.0,
            40.0,
            44.0,
            rgba(0xffffff, 1.0),
            Align::Left,
            "SMOKE TEST 123",
        );
        g.ui.ring(
            g.size.0 as f32 - 80.0,
            80.0,
            40.0,
            8.0,
            0.7,
            rgba(0xf2c230, 1.0),
        );
        let s = g.render(target, dt);
        println!("{s:?}");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App {
        g: None,
        cube: MeshId::procedural(0),
        ball: MeshId::procedural(0),
        font: FontId(0),
    };
    let out = std::env::args().nth(1).unwrap_or("smoke.png".into());
    let r = platform::headless::run(&mut app, (1280, 720), 3, Some(std::path::Path::new(&out)))?;
    println!("frame ms: {:?}", r.frame_ms);
    Ok(())
}
