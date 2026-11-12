//! Dynamic resolution on a real GPU: a client with a governor sees its render scale fall while
//! frames are slow and climb back under vsync-paced frames. The scale is read from the renderer itself
//! (`Gfx::render_scale`) inside the draw callback, so the test checks what was drawn, not what the
//! governor thinks. The test fails (it does not skip) when the machine has no GPU adapter.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::cell::Cell;

use app::{AppError, ClientApp, ClientAssets, Presenter, RenderFrame, ViewAngles};
use common::{Mover, config, profile};
use glam::{Affine3A, Vec3};
use platform::{Gpu, Input, PlatformApp};
use render::{
    Camera, Gfx, GovernorConfig, Layer, Look, Mat, MeshBuilder, MeshId, PrimitiveDesc, PrimitiveId,
    Settings,
};

/// Frame time that is far over the 16.6 ms target: the governor lowers the scale.
const SLOW_DT: f32 = 0.05;
/// Frame time of a perfect 60 Hz display: it meets the target, so the governor probes back up.
const VSYNC_DT: f32 = 0.0167;
const SIZE: (u32, u32) = (256, 256);
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Draws every body as a cube and records the render scale the renderer used for the frame.
#[derive(Default)]
struct ScalePresenter {
    cube: Option<MeshId>,
    seen_scale: Cell<f32>,
    primitive: Option<PrimitiveId>,
}

impl Presenter for ScalePresenter {
    type Command = ();
    type Settings = ();

    fn setup(&mut self, gfx: &mut Gfx, _assets: &mut ClientAssets<'_>) -> Result<(), AppError> {
        self.cube = Some(gfx.add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0)))?);
        Ok(())
    }

    fn command(&mut self, _input: &Input, _look: ViewAngles) -> Result<Self::Command, AppError> {
        Ok(())
    }

    fn grab_mouse(&self) -> bool {
        false
    }

    fn camera(
        &self,
        frame: &RenderFrame,
        alpha: f32,
        look: ViewAngles,
    ) -> Result<Camera, AppError> {
        let entry = frame.transforms().first().expect("one body");
        let body = frame.blended(entry.id, alpha)?.expect("body present");
        Ok(Camera {
            pos: body.translation() + Vec3::new(0.0, 0.0, 4.0),
            yaw: look.yaw(),
            pitch: look.pitch(),
            ..Camera::default()
        })
    }

    fn draw(
        &mut self,
        frame: &RenderFrame,
        alpha: f32,
        gfx: &mut Gfx,
        _assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        self.seen_scale.set(gfx.render_scale());
        let cube = self.cube.ok_or(AppError::ClientNotReady)?;
        for entry in frame.transforms() {
            if let Some(transform) = frame.blended(entry.id, alpha)? {
                let affine = Affine3A::from_mat4(transform.matrix());
                if let Some(primitive) = self.primitive {
                    gfx.scene_mut().set_transform(primitive, affine)?;
                } else {
                    self.primitive = Some(gfx.scene_mut().add(
                        cube,
                        PrimitiveDesc {
                            transform: affine,
                            material: render::Material::Procedural(Look {
                                tint: [0.784, 0.333, 0.239, 1.0],
                                ..Look::default()
                            }),
                            layer: Layer::World,
                            casts_shadow: true,
                        },
                    )?);
                }
            }
        }
        Ok(())
    }
}

/// Runs `frames` frames of `dt` seconds each through the platform entry point.
fn run_frames(
    client: &mut ClientApp<Mover, ScalePresenter>,
    gpu: &Gpu,
    view: &wgpu::TextureView,
    input: &mut Input,
    frames: u32,
    dt: f32,
) {
    for _ in 0..frames {
        PlatformApp::frame(client, view, input, dt);
        input.end_frame();
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes the frame");
    }
    assert!(
        client.take_error().is_none(),
        "the client stopped with an error"
    );
}

#[test]
fn the_render_scale_drops_under_slow_frames_and_recovers_under_vsync() {
    let gpu = Gpu::new(wgpu::Instance::default(), None)
        .expect("governor headless test needs a GPU adapter");
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("governor-headless"),
        size: wgpu::Extent3d {
            width: SIZE.0,
            height: SIZE.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let governor = GovernorConfig::new(16.6, 0.6, 1.0).expect("valid governor");
    let mut client = ClientApp::new(
        Mover,
        &(),
        ScalePresenter::default(),
        &config(),
        &profile(),
        Settings::default(),
    )
    .expect("client builds")
    .with_governor(governor)
    .expect("governor starts at the settings scale");
    client.init(&gpu, FORMAT, SIZE);
    assert!(client.take_error().is_none(), "the renderer did not build");

    let mut input = Input::default();
    input.screen = glam::Vec2::new(SIZE.0 as f32, SIZE.1 as f32);

    // Eight steps of 0.05 take 1.0 down to the 0.6 floor, one full 30-frame window each. The
    // renderer must report the lowered scale from the draw of the frame after each change.
    run_frames(&mut client, &gpu, &view, &mut input, 300, SLOW_DT);
    let lowered = client.presenter().seen_scale.get();
    assert!(
        (lowered - 0.6).abs() < 1e-4,
        "slow frames should lower the render scale to 0.6, got {lowered}"
    );

    // Under vsync a frame never beats the refresh interval, so recovery is by probing: five
    // windows per step and eight steps, so 1500 frames is enough to reach 1.0.
    run_frames(&mut client, &gpu, &view, &mut input, 1500, VSYNC_DT);
    let recovered = client.presenter().seen_scale.get();
    assert!(
        (recovered - 1.0).abs() < 1e-4,
        "vsync-paced frames should probe the render scale back to 1.0, got {recovered}"
    );
}
