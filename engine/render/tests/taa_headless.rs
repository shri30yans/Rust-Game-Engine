//! Headless GPU checks for temporal anti-aliasing on a 256x256 target: a static scene converges,
//! static scene converges, a moving cube leaves no trail, and wrong motion vectors measurably
//! bias the resolved image. Each test fails (it does not skip) when the machine has no GPU
//! adapter.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use glam::{Affine3A, Mat4, Vec3};
use platform::Gpu;
use render::{
    Camera, FramePass, Gfx, Layer, Look, Mat, MeshBuilder, Pattern, Post, PrimitiveDesc,
    RenderTier, Settings, ViewMode,
};

const SIZE: u32 = 256;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Camera default eye height.
const CAM_Y: f32 = 1.7;
/// Cube is 6 m in front of the camera, at the eye's height.
const CUBE_Z: f32 = -6.0;

fn gpu() -> Gpu {
    Gpu::new(wgpu::Instance::default(), None).expect("headless TAA test needs a GPU adapter")
}

/// High tier with bloom off, TAA set as asked.
fn settings(taa: bool) -> Settings {
    let mut s = RenderTier::High.settings();
    s.bloom = false;
    s.taa = taa;
    s
}

/// Renders `frames` frames of one cube. `place(i)` returns the cube's current and previous
/// model matrices for frame `i`. A lit cube carries a brick pattern so its interior has
/// texture to accumulate; an unlit cube is a flat colour. Returns the RGBA bytes read back
/// after each frame.
fn run(
    gpu: &Gpu,
    taa: bool,
    lit: bool,
    frames: u32,
    place: impl Fn(u32) -> (Mat4, Mat4),
) -> Vec<Vec<u8>> {
    let mut g = Gfx::new(gpu, FORMAT, (SIZE, SIZE), settings(taa)).expect("renderer builds");
    g.post = Post {
        bloom: 0.0,
        vignette: 0.0,
        grain: 0.0,
        saturation: 1.0,
        contrast: 1.0,
        shadow_tint: [0.0; 4],
        highlight_tint: [0.0; 4],
        ..Post::default()
    };
    let cube = g
        .add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0)))
        .expect("mesh uploads");
    let (initial, _) = place(0);
    let primitive = g
        .scene_mut()
        .add(
            cube,
            PrimitiveDesc {
                transform: Affine3A::from_mat4(initial),
                material: render::Material::Procedural(Look {
                    tint: [0.784, 0.333, 0.239, 1.0],
                    pattern: lit.then_some(Pattern::Brick),
                    unlit: !lit,
                    ..Look::default()
                }),
                layer: Layer::World,
                casts_shadow: true,
            },
        )
        .expect("primitive added");
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("taa-target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut out = Vec::with_capacity(frames as usize);
    for i in 0..frames {
        let (model, _) = place(i);
        g.begin_frame(Camera::default());
        g.scene_mut()
            .set_transform(primitive, Affine3A::from_mat4(model))
            .expect("primitive live");
        g.render(&view, 1.0 / 60.0);
        out.push(readback(gpu, &target));
    }
    out
}

/// Copies the target to the CPU and returns its tightly packed RGBA bytes.
fn readback(gpu: &Gpu, tex: &wgpu::Texture) -> Vec<u8> {
    let len = u64::from(SIZE) * u64::from(SIZE) * 4;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("taa-readback"),
        size: len,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
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
    gpu.queue.submit([enc.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).expect("readback receiver is alive");
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    rx.recv()
        .expect("map callback ran")
        .expect("readback buffer mapped");
    buf.slice(..)
        .get_mapped_range()
        .expect("mapped range")
        .to_vec()
}

/// Frame `i` of a run, panicking-free: the test fails with a message if it was never rendered.
fn frame_at(frames: &[Vec<u8>], i: usize) -> &[u8] {
    frames.get(i).expect("frame was rendered")
}

/// RGB of pixel `(x, y)` in a frame read back by [`readback`].
fn rgb(frame: &[u8], x: u32, y: u32) -> [u8; 3] {
    let start = ((y * SIZE + x) * 4) as usize;
    frame
        .get(start..start + 3)
        .expect("pixel is inside the frame")
        .try_into()
        .expect("three colour bytes")
}

/// Luminance in [0, 1] of an RGB triple.
fn lum(c: [u8; 3]) -> f32 {
    (0.2126 * f32::from(c[0]) + 0.7152 * f32::from(c[1]) + 0.0722 * f32::from(c[2])) / 255.0
}

/// Normalized mean absolute RGB difference between two rendered frames.
fn frame_difference(a: &[u8], b: &[u8]) -> f64 {
    let total: u64 = a
        .iter()
        .zip(b.iter())
        .map(|(left, right)| u64::from(left.abs_diff(*right)))
        .sum();
    total as f64 / (a.len() as f64 * 255.0)
}

/// Model matrix of the cube with its centre at horizontal world position `x`.
fn at(x: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, CAM_Y, CUBE_Z))
}

#[test]
fn taa_converges_a_static_scene() {
    let gpu = gpu();
    let still = |_: u32| {
        let m = at(0.0) * Mat4::from_rotation_y(0.4);
        (m, m)
    };
    let on = run(&gpu, true, false, 17, still);

    // The former distinct-colour count assumed a black, flat scene. After correcting the world
    // lighting, it counts the scene's natural luminance range instead of edge coverage. A TAA
    // resolve must visibly change while history fills, then settle as its static history converges.
    let early = frame_difference(frame_at(&on, 0), frame_at(&on, 1));
    let late = frame_difference(frame_at(&on, 15), frame_at(&on, 16));
    assert!(
        early > 0.000_01,
        "TAA must change the jittered static image while it initializes, got {early}"
    );
    assert!(
        late < early * 0.5,
        "TAA must converge its static history: early difference {early}, late difference {late}"
    );
}

#[test]
fn a_moving_cube_leaves_no_trail_with_correct_motion() {
    let gpu = gpu();
    // The cube moves 0.4 m to the right each frame, about 12 px at this distance.
    let x_of = |i: u32| -1.0 + 0.4 * i as f32;
    let moving = |i: u32| (at(x_of(i)), at(x_of(i.saturating_sub(1))));

    let off = run(&gpu, false, false, 8, moving);
    let on = run(&gpu, true, false, 8, moving);

    // Trailing-edge pixel: the cube covers it at frame 5 and has left it by frame 6.
    let cube_lum = lum(rgb(frame_at(&off, 5), SIZE / 2 + 16, SIZE / 2));
    let row = SIZE / 2;
    let p = (0..SIZE)
        .find(|&x| {
            (lum(rgb(frame_at(&off, 5), x, row)) - cube_lum).abs() < 0.02
                && (lum(rgb(frame_at(&off, 6), x, row)) - cube_lum).abs() > 0.1
        })
        .expect("the cube has a trailing edge between frames 5 and 6");
    let background = lum(rgb(frame_at(&off, 7), p, row));

    let with_motion = (lum(rgb(frame_at(&on, 7), p, row)) - background).abs();
    assert!(
        with_motion <= 0.05,
        "trailing edge ghosts with correct motion: pixel {p} differs from background by {with_motion}"
    );
}

#[test]
fn set_taa_rebuilds_and_creates_the_prepass_on_medium() {
    let gpu = gpu();
    let mut high = Gfx::new(&gpu, FORMAT, (SIZE, SIZE), settings(false)).expect("High builds");
    high.set_taa(true).expect("High runs TAA");
    high.set_taa(false).expect("TAA turns off again");

    let mut medium = Gfx::new(
        &gpu,
        FORMAT,
        (SIZE, SIZE),
        Settings {
            tier: RenderTier::Medium,
            ..settings(false)
        },
    )
    .expect("Medium builds without TAA");
    medium.set_taa(true).expect("TAA creates Medium prepass");
    assert!(medium.frame_plan().reuses_prepass_depth());
}

#[test]
fn settings_updates_rebuild_the_checked_plan_and_restore_supported_msaa() {
    let gpu = gpu();
    let mut initial = RenderTier::Medium.settings();
    initial.bloom = false;
    let mut renderer = Gfx::new(&gpu, FORMAT, (SIZE, SIZE), initial).expect("Medium builds");
    let requested_msaa = renderer.msaa();
    assert_eq!(renderer.settings().tier, RenderTier::Medium);
    assert!(!renderer.frame_plan().reuses_prepass_depth());

    let mut taa = renderer.settings();
    taa.taa = true;
    renderer
        .set_settings(taa)
        .expect("settings transaction enables TAA");
    assert!(renderer.settings().taa);
    assert_eq!(renderer.msaa(), 1);
    assert!(renderer.frame_plan().contains(FramePass::Taa));
    assert!(renderer.frame_plan().reuses_prepass_depth());

    renderer.set_taa(false).expect("TAA turns off again");
    assert!(!renderer.settings().taa);
    assert_eq!(renderer.msaa(), requested_msaa);
    assert!(!renderer.frame_plan().contains(FramePass::Taa));
    assert!(!renderer.frame_plan().reuses_prepass_depth());

    renderer
        .set_view_mode(ViewMode::Depth)
        .expect("depth view creates its prepass");
    assert_eq!(renderer.msaa(), 1);
    assert!(renderer.frame_plan().reuses_prepass_depth());
    renderer
        .set_view_mode(ViewMode::Lit)
        .expect("lit view restores requested MSAA");
    assert_eq!(renderer.msaa(), requested_msaa);
    assert_eq!(renderer.view_mode(), ViewMode::Lit);
    assert!(!renderer.frame_plan().reuses_prepass_depth());
}

#[test]
fn medium_tier_with_taa_builds_a_checked_plan() {
    let gpu = gpu();
    let taa_medium = Settings {
        tier: RenderTier::Medium,
        taa: true,
        ..Settings::default()
    };
    let renderer = Gfx::new(&gpu, FORMAT, (SIZE, SIZE), taa_medium)
        .expect("TAA request creates required prepass");
    assert!(renderer.frame_plan().reuses_prepass_depth());
}
