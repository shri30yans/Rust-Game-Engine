//! One end-to-end frame on a real GPU: the client builds the renderer, runs ticks, extracts,
//! blends and draws a lit cube, and the PNG read back from the headless target shows it.
//! The test fails (it does not skip) when the machine has no GPU adapter.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use app::{AppError, ClientApp, ClientAssets, Presenter, RenderFrame, ViewAngles};
use common::{Mover, config, profile};
use engine_world::World;
use glam::{Affine3A, Vec3};
use platform::Input;
use render::golden::Image;
use render::{
    Camera, Gfx, Layer, Look, Mat, MeshBuilder, MeshId, PrimitiveDesc, PrimitiveId, Settings,
};

/// Draws every body as a cube and keeps the camera level with the first body.
#[derive(Default)]
struct CubePresenter {
    cube: Option<MeshId>,
    primitive: Option<PrimitiveId>,
    scene_empty_after_travel: bool,
}

impl Presenter for CubePresenter {
    type Command = ();
    type Settings = ();

    fn setup(&mut self, gfx: &mut Gfx, _assets: &mut ClientAssets<'_>) -> Result<(), AppError> {
        self.cube = Some(gfx.add_mesh(&MeshBuilder::unit_cube(Mat::hex(0xffffff, 0.7, 0.0)))?);
        Ok(())
    }

    fn command(&mut self, _input: &Input, _look: ViewAngles) -> Result<Self::Command, AppError> {
        Ok(())
    }

    fn on_travel(&mut self, _world: &World, gfx: Option<&mut Gfx>) -> Result<(), AppError> {
        if let Some(primitive) = self.primitive.take() {
            let renderer = gfx.ok_or(AppError::ClientNotReady)?;
            renderer.scene_mut().remove(primitive)?;
            self.scene_empty_after_travel = renderer.scene_mut().is_empty();
        }
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

/// The RGB value of pixel `(x, y)`.
fn rgb_at(image: &Image, x: u32, y: u32) -> [u8; 3] {
    let index = usize::try_from(y * image.dimensions.0 + x).expect("pixel index fits in usize");
    let &[red, green, blue, _alpha] = image.pixels.get(index).expect("pixel is inside the image");
    [red, green, blue]
}

#[test]
fn a_lit_cube_is_visible_in_the_rendered_frame() {
    let mut client = ClientApp::new(
        Mover,
        &(),
        CubePresenter::default(),
        &config(),
        &profile(),
        Settings::default(),
    )
    .expect("client builds");
    let path = std::env::temp_dir().join(format!("engine-app-headless-{}.png", std::process::id()));
    platform::headless::run(&mut client, (256, 256), 3, Some(&path))
        .expect("headless render needs a GPU adapter");
    assert!(client.take_error().is_none(), "the client stored an error");

    let image = Image::read(&path).expect("screenshot decodes");
    let (width, height) = image.dimensions;
    let centre = rgb_at(&image, width / 2, height / 2);
    let corner = rgb_at(&image, 2, 2);
    let difference: u32 = centre
        .iter()
        .zip(corner.iter())
        .map(|(a, b)| u32::from(a.abs_diff(*b)))
        .sum();
    assert!(
        difference > 60,
        "centre {centre:?} and corner {corner:?} look the same: the cube was not drawn"
    );
    client
        .travel(&())
        .expect("travel clears retained primitives");
    assert!(
        client.presenter().scene_empty_after_travel,
        "a retained primitive from the old world remained after travel"
    );
    std::fs::remove_file(&path).expect("remove the screenshot");
}
