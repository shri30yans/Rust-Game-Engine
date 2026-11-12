//! The scene presenter: draws every frame entity that carries a `Visual`, with the mesh its shape
//! needs and the style its `Look` names.
//!
//! The presenter is a helper a game's own presenter calls from `setup` and `draw`; it does not
//! handle input or the camera, which are the game's. The three unit meshes are built once in
//! [`ScenePresenter::setup`]. Each entity is scaled by its `Transform`, whose scale is the shape's
//! full size, so a cuboid or ramp uses the unit cube, a sphere the radius-0.5 sphere, and a
//! capsule the radius-0.5 capsule stretched along Y by half. The capsule stretch makes its caps
//! ellipsoidal when the half height differs from the radius. Physics uses the exact shape.
//!
//! Invariants: a visual whose entity has no transform in the same frame is an error, because the
//! extractor always copies both; a surface's style comes only from its `Look`, so one material
//! looks the same wherever it is placed; drawing before `setup` is an error, not a silent skip.

use engine_assets::format::{AssetId, AssetIndex, MeshPack};
use engine_assets::{AssetPath, Handle};
use engine_scene::{Look, MaterialKind, MeshKind};
use engine_world::NetId;
use glam::{Affine3A, Mat4, Vec3};
use render::{
    Gfx, Layer, Mat, Material, MaterialId, MeshBuilder, MeshId, Pattern, PrimitiveDesc,
    PrimitiveId, srgb,
};
use std::collections::HashMap;

use crate::asset_uploader::ClientAssets;
use crate::asset_uploader::UploadStatus;
use crate::blend::blend;
use crate::error::AppError;
use crate::extract::RenderFrame;

/// Radius of the unit sphere and capsule meshes, and the half size of the unit cube.
const UNIT_RADIUS: f32 = 0.5;
/// Segments around each unit sphere and capsule.
const SEGMENTS: u32 = 24;
/// Rings from pole to pole of each unit sphere.
const RINGS: u32 = 16;
/// Y scale that turns the unit capsule (half height plus radius of 1.0) into a capsule whose
/// total half extent is the entity's full Y size over two.
const CAPSULE_Y_SCALE: f32 = 0.5;
/// Emissive strength of an `Emissive` surface.
const EMISSIVE_STRENGTH: f32 = 2.0;

/// The unit meshes that draw scene entities.
struct Meshes {
    cube: MeshId,
    sphere: MeshId,
    capsule: MeshId,
}

#[derive(Clone, Copy)]
struct PrimitiveState {
    primitive: PrimitiveId,
    mesh: MeshId,
    material: Material,
    transform_bits: [u32; 16],
}

struct CookedNode {
    pack: Handle<MeshPack>,
    node: u32,
}

#[derive(Clone, Copy)]
struct CookedDraw {
    mesh: MeshId,
    material: MaterialId,
}

enum MeshSet<'a> {
    Procedural(&'a [MeshId]),
    Cooked(&'a [CookedDraw]),
}

impl MeshSet<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Procedural(meshes) => meshes.len(),
            Self::Cooked(meshes) => meshes.len(),
        }
    }

    fn get(&self, index: usize) -> Option<(MeshId, Option<MaterialId>)> {
        match self {
            Self::Procedural(meshes) => meshes.get(index).copied().map(|mesh| (mesh, None)),
            Self::Cooked(meshes) => meshes
                .get(index)
                .copied()
                .map(|draw| (draw.mesh, Some(draw.material))),
        }
    }
}

/// Draws scene visuals. Build it once, call [`setup`](Self::setup) before the first draw.
#[derive(Default)]
pub struct ScenePresenter {
    meshes: Option<Meshes>,
    primitives: HashMap<NetId, Vec<PrimitiveState>>,
    cooked_nodes: HashMap<AssetId, CookedNode>,
    cooked_draws: HashMap<AssetId, Vec<CookedDraw>>,
    cooked_hashes: HashMap<AssetId, u64>,
    removed: Vec<NetId>,
}

/// The colour and surface response of a visual, as the renderer takes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Linear RGB colour.
    pub colour: [f32; 3],
    /// Roughness in `[0, 1]`.
    pub rough: f32,
    /// Metallic amount in `[0, 1]`.
    pub metal: f32,
    /// Surface pattern.
    pub pattern: Pattern,
    /// Self-lit strength; zero for an unlit surface.
    pub emissive: f32,
}

/// The style a [`Look`] draws with. An explicit `tint` replaces the material's colour and keeps
/// the material's surface response.
#[must_use]
pub fn style_of(look: Look) -> Style {
    let base = material_style(look.material);
    match look.tint {
        Some(hex) => Style {
            colour: srgb(hex),
            ..base
        },
        None => base,
    }
}

fn material_style(material: MaterialKind) -> Style {
    let plain = |hex: u32, rough: f32, metal: f32| Style {
        colour: srgb(hex),
        rough,
        metal,
        pattern: Pattern::None,
        emissive: 0.0,
    };
    match material {
        MaterialKind::Floor | MaterialKind::Wall => plain(0x7d_8590, 0.8, 0.0),
        MaterialKind::Crate => plain(0xe0_7b39, 0.8, 0.0),
        MaterialKind::Concrete => Style {
            pattern: Pattern::Concrete,
            ..plain(0x9a_9a96, 0.9, 0.0)
        },
        MaterialKind::Brick => Style {
            pattern: Pattern::Brick,
            ..plain(0x9b_5d3c, 0.9, 0.0)
        },
        MaterialKind::Wood => Style {
            pattern: Pattern::Wood,
            ..plain(0x8b_5a2b, 0.7, 0.0)
        },
        MaterialKind::Metal => plain(0xb0_b6bd, 0.3, 1.0),
        MaterialKind::Glass => plain(0xa8_d8ea, 0.1, 0.0),
        MaterialKind::Emissive => Style {
            emissive: EMISSIVE_STRENGTH,
            ..plain(0xff_f3c4, 0.5, 0.0)
        },
        MaterialKind::Custom {
            colour_hex,
            roughness,
            metallic,
        } => plain(colour_hex, roughness, metallic),
    }
}

/// The model matrix that draws a unit mesh of `kind` for an entity whose transform is `matrix`.
#[must_use]
pub fn draw_matrix(kind: MeshKind, matrix: Mat4) -> Mat4 {
    match kind {
        MeshKind::Box | MeshKind::Sphere | MeshKind::Cooked(_) => matrix,
        MeshKind::Capsule => matrix * Mat4::from_scale(Vec3::new(1.0, CAPSULE_Y_SCALE, 1.0)),
    }
}

impl ScenePresenter {
    /// Builds the three unit meshes on the GPU. Call once, before the first draw.
    pub fn setup(&mut self, gfx: &mut Gfx) -> Result<(), AppError> {
        let white = Mat::new([1.0; 3], 0.8, 0.0);
        let cube = gfx.add_mesh(&MeshBuilder::unit_cube(white))?;
        let mut sphere_builder = MeshBuilder::new();
        sphere_builder.sphere(Vec3::ZERO, Vec3::splat(UNIT_RADIUS), SEGMENTS, RINGS, white);
        let sphere = gfx.add_mesh(&sphere_builder)?;
        let mut capsule_builder = MeshBuilder::new();
        capsule_builder.capsule(Vec3::ZERO, UNIT_RADIUS, UNIT_RADIUS, SEGMENTS, white);
        let capsule = gfx.add_mesh(&capsule_builder)?;
        self.meshes = Some(Meshes {
            cube,
            sphere,
            capsule,
        });
        self.primitives.reserve(4096);
        self.removed.reserve(4096);
        Ok(())
    }

    /// Loads cooked packs referenced by this frame into the client's shared asset store.
    ///
    /// # Errors
    /// Returns an error when the index, path or store load is invalid.
    pub fn load_frame_assets(
        &mut self,
        frame: &RenderFrame,
        assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        let asset_ids = frame
            .visuals()
            .iter()
            .filter_map(|drawn| match drawn.visual.mesh {
                MeshKind::Cooked(asset_id) => Some(asset_id),
                MeshKind::Box | MeshKind::Sphere | MeshKind::Capsule => None,
            });
        self.load_asset_ids(asset_ids, assets)
    }

    fn load_asset_ids(
        &mut self,
        asset_ids: impl Iterator<Item = AssetId>,
        assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        let missing = asset_ids
            .filter(|asset_id| !self.cooked_nodes.contains_key(asset_id))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        let index_path = AssetPath::new("content/cooked/index.ron")
            .map_err(|error| AppError::CookedAsset(error.to_string()))?;
        let index_text = assets
            .content_text(&index_path)
            .map_err(|error| AppError::CookedAsset(error.to_string()))?;
        let index = ron::from_str::<AssetIndex>(index_text)
            .map_err(|error| AppError::CookedAsset(format!("{}: {error}", index_path)))?;
        for asset_id in missing {
            let location = index.get(&asset_id).ok_or_else(|| {
                AppError::CookedAsset(format!("asset {} is missing from index", asset_id.get()))
            })?;
            let node = location.node.ok_or_else(|| {
                AppError::CookedAsset(format!(
                    "asset {} points at a source file, not a node",
                    asset_id.get()
                ))
            })?;
            let pack_path = AssetPath::new(format!("content/cooked/{}", location.pack))
                .map_err(|error| AppError::CookedAsset(error.to_string()))?;
            let pack = assets
                .load(pack_path)
                .map_err(|error| AppError::CookedAsset(error.to_string()))?;
            self.cooked_nodes
                .insert(asset_id, CookedNode { pack, node });
        }
        Ok(())
    }

    fn refresh_cooked_draws(&mut self, assets: &ClientAssets<'_>) -> Result<(), AppError> {
        let mut refreshed = Vec::new();
        for (asset_id, node) in &self.cooked_nodes {
            let Some(pack) = assets
                .current(&node.pack)
                .map_err(|error| AppError::CookedAsset(error.to_string()))?
            else {
                continue;
            };
            let UploadStatus::Ready(gpu) = assets.uploader.status(&node.pack) else {
                continue;
            };
            if self.cooked_hashes.get(asset_id) == Some(&pack.source_hash) {
                continue;
            }
            let mut draws = Vec::new();
            for (mesh_index, mesh) in pack.meshes.iter().enumerate() {
                if mesh.node != node.node {
                    continue;
                }
                let Some(mesh_id) = gpu.meshes.get(mesh_index).copied() else {
                    continue;
                };
                let Some(material_index) = usize::try_from(mesh.material).ok() else {
                    continue;
                };
                let Some(material_id) = gpu.materials.get(material_index).copied() else {
                    continue;
                };
                draws.push(CookedDraw {
                    mesh: mesh_id,
                    material: material_id,
                });
            }
            if draws.is_empty() {
                return Err(AppError::MissingCookedMesh(*asset_id));
            }
            refreshed.push((*asset_id, pack.source_hash, draws));
        }
        for (asset_id, source_hash, draws) in refreshed {
            self.cooked_draws.insert(asset_id, draws);
            self.cooked_hashes.insert(asset_id, source_hash);
        }
        Ok(())
    }

    /// Draws every visual in `frame`, blending each transform at `alpha`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ClientNotReady`] before `setup`, [`AppError::MissingFrameEntity`] when
    /// a visual has no transform in the frame, and [`AppError::BlendAlpha`] for an alpha outside
    /// `[0, 1]`.
    pub fn draw(
        &mut self,
        frame: &RenderFrame,
        alpha: f32,
        gfx: &mut Gfx,
        assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        self.draw_selected(frame, alpha, gfx, assets, false)
    }

    /// Draws only cooked mesh instances from `frame`, for games that render their other actors
    /// through a game-owned presenter.
    ///
    /// # Errors
    /// Returns the same readiness, entity and interpolation errors as [`Self::draw`].
    pub fn draw_cooked(
        &mut self,
        frame: &RenderFrame,
        alpha: f32,
        gfx: &mut Gfx,
        assets: &mut ClientAssets<'_>,
    ) -> Result<(), AppError> {
        self.draw_selected(frame, alpha, gfx, assets, true)
    }

    fn draw_selected(
        &mut self,
        frame: &RenderFrame,
        alpha: f32,
        gfx: &mut Gfx,
        assets: &mut ClientAssets<'_>,
        cooked_only: bool,
    ) -> Result<(), AppError> {
        self.refresh_cooked_draws(assets)?;
        let meshes = self.meshes.as_ref().ok_or(AppError::ClientNotReady)?;
        self.removed.clear();
        self.removed
            .extend(self.primitives.keys().copied().filter(|id| {
                frame.visual_entry(*id).is_none_or(|entry| {
                    cooked_only && !matches!(entry.visual.mesh, MeshKind::Cooked(_))
                })
            }));
        for id in self.removed.drain(..) {
            if let Some(states) = self.primitives.remove(&id) {
                for state in states {
                    gfx.scene_mut().remove(state.primitive)?;
                }
            }
        }
        for drawn in frame.visuals() {
            if cooked_only && !matches!(drawn.visual.mesh, MeshKind::Cooked(_)) {
                continue;
            }
            let entry = frame
                .transform_entry(drawn.id)
                .ok_or(AppError::MissingFrameEntity(drawn.id))?;
            let transform = blend(&entry.previous, &entry.current, alpha)?;
            let kind = drawn.visual.mesh;
            let procedural_meshes;
            let meshes_for_entity = match kind {
                MeshKind::Box => {
                    procedural_meshes = [meshes.cube];
                    MeshSet::Procedural(&procedural_meshes[..])
                }
                MeshKind::Sphere => {
                    procedural_meshes = [meshes.sphere];
                    MeshSet::Procedural(&procedural_meshes[..])
                }
                MeshKind::Capsule => {
                    procedural_meshes = [meshes.capsule];
                    MeshSet::Procedural(&procedural_meshes[..])
                }
                MeshKind::Cooked(asset_id) => {
                    let Some(draws) = self.cooked_draws.get(&asset_id) else {
                        continue;
                    };
                    MeshSet::Cooked(draws)
                }
            };
            let style = style_of(drawn.visual.look);
            // Last tick's matrix drives motion vectors, so TAA does not ghost moving objects.
            let transform = Affine3A::from_mat4(draw_matrix(kind, transform.matrix()));
            let transform_bits = Mat4::from(transform).to_cols_array().map(f32::to_bits);
            let look = render::Look {
                tint: [style.colour[0], style.colour[1], style.colour[2], 1.0],
                emissive: style.emissive,
                roughness: Some(style.rough),
                metalness: Some(style.metal),
                pattern: Some(style.pattern),
                object_space_pattern: false,
                unlit: false,
            };
            let current_matches = self.primitives.get(&drawn.id).is_some_and(|states| {
                states.len() == meshes_for_entity.len()
                    && states.iter().enumerate().all(|(index, state)| {
                        meshes_for_entity
                            .get(index)
                            .is_some_and(|(mesh, _)| state.mesh == mesh)
                    })
            });
            if current_matches {
                if let Some(states) = self.primitives.get_mut(&drawn.id) {
                    for state in states {
                        if state.transform_bits != transform_bits {
                            gfx.scene_mut().set_transform(state.primitive, transform)?;
                            state.transform_bits = transform_bits;
                        }
                        let target_material = match kind {
                            MeshKind::Cooked(asset_id) => self
                                .cooked_draws
                                .get(&asset_id)
                                .and_then(|draws| draws.iter().find(|draw| draw.mesh == state.mesh))
                                .map(|draw| Material::Textured(draw.material))
                                .unwrap_or(Material::Procedural(look)),
                            MeshKind::Box | MeshKind::Sphere | MeshKind::Capsule => {
                                Material::Procedural(look)
                            }
                        };
                        if state.material != target_material {
                            gfx.scene_mut()
                                .set_material(state.primitive, target_material)?;
                            state.material = target_material;
                        }
                    }
                }
                continue;
            }
            if let Some(old_states) = self.primitives.remove(&drawn.id) {
                for state in old_states {
                    gfx.scene_mut().remove(state.primitive)?;
                }
            }
            let mut states = Vec::with_capacity(meshes_for_entity.len());
            for index in 0..meshes_for_entity.len() {
                let Some((mesh, cooked_material)) = meshes_for_entity.get(index) else {
                    continue;
                };
                let material = cooked_material
                    .map(Material::Textured)
                    .unwrap_or(Material::Procedural(look));
                let primitive = gfx.scene_mut().add(
                    mesh,
                    PrimitiveDesc {
                        transform,
                        material,
                        layer: Layer::World,
                        casts_shadow: true,
                    },
                )?;
                states.push(PrimitiveState {
                    primitive,
                    mesh,
                    material,
                    transform_bits,
                });
            }
            self.primitives.insert(drawn.id, states);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn look(material: MaterialKind, tint: Option<u32>) -> Look {
        Look { material, tint }
    }

    #[test]
    fn every_material_has_a_style_with_values_in_range() {
        let materials = [
            MaterialKind::Floor,
            MaterialKind::Wall,
            MaterialKind::Concrete,
            MaterialKind::Brick,
            MaterialKind::Metal,
            MaterialKind::Wood,
            MaterialKind::Crate,
            MaterialKind::Glass,
            MaterialKind::Emissive,
        ];
        for material in materials {
            let style = style_of(look(material, None));
            assert!((0.0..=1.0).contains(&style.rough), "{material:?}");
            assert!((0.0..=1.0).contains(&style.metal), "{material:?}");
            assert!(style.emissive >= 0.0, "{material:?}");
        }
    }

    #[test]
    fn walls_and_floors_share_the_sandbox_grey() {
        assert_eq!(
            style_of(look(MaterialKind::Floor, None)).colour,
            srgb(0x7d_8590)
        );
        assert_eq!(
            style_of(look(MaterialKind::Wall, None)).colour,
            srgb(0x7d_8590)
        );
    }

    #[test]
    fn only_emissive_surfaces_glow() {
        assert!(style_of(look(MaterialKind::Emissive, None)).emissive > 0.0);
        assert_eq!(style_of(look(MaterialKind::Crate, None)).emissive, 0.0);
    }

    #[test]
    fn custom_material_uses_its_own_colour_and_response() {
        let style = style_of(look(
            MaterialKind::Custom {
                colour_hex: 0x33_6699,
                roughness: 0.4,
                metallic: 0.25,
            },
            None,
        ));
        assert_eq!(style.colour, srgb(0x33_6699));
        assert_eq!(style.rough, 0.4);
        assert_eq!(style.metal, 0.25);
    }

    #[test]
    fn tint_replaces_the_colour_and_keeps_the_surface_response() {
        let plain = style_of(look(MaterialKind::Metal, None));
        let tinted = style_of(look(MaterialKind::Metal, Some(0xff_0000)));
        assert_eq!(tinted.colour, srgb(0xff_0000));
        assert_ne!(tinted.colour, plain.colour);
        assert_eq!(tinted.rough, plain.rough);
        assert_eq!(tinted.metal, plain.metal);
    }

    #[test]
    fn capsule_is_stretched_along_y_and_the_others_are_not() {
        let matrix = Mat4::from_scale(Vec3::new(2.0, 4.0, 2.0));
        assert_eq!(draw_matrix(MeshKind::Box, matrix), matrix);
        assert_eq!(draw_matrix(MeshKind::Sphere, matrix), matrix);
        let capsule = draw_matrix(MeshKind::Capsule, matrix);
        let scale = capsule.to_scale_rotation_translation().0;
        assert!(
            (scale - Vec3::new(2.0, 2.0, 2.0)).length() < 1e-6,
            "{scale}"
        );
    }
}
