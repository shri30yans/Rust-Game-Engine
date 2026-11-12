//! Retained primitive storage for `Gfx`.
//!
//! Primitive state lives in parallel arrays so culling walks only the fields it
//! needs. Packed GPU records are cached per slot and rebuilt only by mutations.

use glam::{Affine3A, Mat4, Vec3, Vec4};

use crate::gfx::{
    FLAG_OBJECT_SPACE, FLAG_UNLIT, Instance, MAX_INSTANCES, MaterialId, PreviousTransform,
    sphere_visible_in_planes,
};
use crate::{MeshId, Pattern, RenderError};

/// Stable handle to a retained primitive. Removal invalidates every copy of its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrimitiveId {
    index: u32,
    generation: u32,
}

impl PrimitiveId {
    /// Slot index in the retained scene.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }
    /// Slot generation captured when the primitive was added.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// Render path for a retained primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// Lit world geometry.
    World,
    /// Camera-space first-person geometry.
    ViewModel,
    /// Additive glow shell.
    Glow,
}

/// Typed shading options stored per primitive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Linear RGBA tint multiplier.
    pub tint: [f32; 4],
    /// Emissive strength.
    pub emissive: f32,
    /// Optional roughness override.
    pub roughness: Option<f32>,
    /// Optional metalness override.
    pub metalness: Option<f32>,
    /// Optional procedural pattern override.
    pub pattern: Option<Pattern>,
    /// Pattern evaluation sticks to object space when enabled.
    pub object_space_pattern: bool,
    /// Skip lighting and shading when enabled.
    pub unlit: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            tint: [1.0; 4],
            emissive: 0.0,
            roughness: None,
            metalness: None,
            pattern: None,
            object_space_pattern: false,
            unlit: false,
        }
    }
}

/// Shading source selected per retained primitive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Material {
    /// Vertex-authored procedural material for greybox and generated geometry.
    Procedural(Look),
    /// Shared textured PBR material owned by `Gfx`.
    Textured(MaterialId),
}

/// Initial state for one retained primitive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrimitiveDesc {
    /// Object-to-world affine transform.
    pub transform: Affine3A,
    /// Per-primitive material source.
    pub material: Material,
    /// Render layer.
    pub layer: Layer,
    /// Whether the primitive contributes to sun shadows.
    pub casts_shadow: bool,
}

/// Visible counts produced by one retained-scene cull.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CullStats {
    /// World primitives rejected by the camera frustum.
    pub culled: u32,
    /// World primitives visible to the main pass.
    pub visible_main: u32,
    /// World primitives visible to each shadow cascade.
    pub visible_shadow: [u32; 2],
}

/// Read-only slot view used by the allocation-free culling loop.
#[derive(Clone, Copy)]
pub(crate) struct SceneSlot {
    pub(crate) live: bool,
    pub(crate) mesh: MeshId,
    pub(crate) material: Material,
    pub(crate) layer: Layer,
    pub(crate) casts_shadow: bool,
    pub(crate) visible: bool,
    pub(crate) center: Vec3,
    pub(crate) radius: f32,
}

/// CPU side of the retained scene. Culling data is structure-of-arrays; GPU records are
/// cached by slot and mutations enqueue only the affected slot for upload.
pub struct RenderScene {
    generations: Vec<u32>,
    live: Vec<bool>,
    meshes: Vec<MeshId>,
    transforms: Vec<Affine3A>,
    previous_transforms: Vec<Affine3A>,
    materials: Vec<Material>,
    layers: Vec<Layer>,
    casts_shadows: Vec<bool>,
    visible: Vec<bool>,
    centers: Vec<Vec3>,
    radii: Vec<f32>,
    records: Vec<Instance>,
    previous_records: Vec<PreviousTransform>,
    dirty_flags: Vec<bool>,
    moved_flags: Vec<bool>,
    mesh_radii: Vec<Option<(MeshId, f32)>>,
    material_ids: Vec<Option<MaterialId>>,
    free: Vec<u32>,
    dirty: Vec<u32>,
    previous_dirty: Vec<u32>,
    moved: Vec<u32>,
    alive: usize,
}

impl RenderScene {
    /// Creates a scene with storage for `capacity` primitive slots.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            generations: Vec::with_capacity(capacity),
            live: Vec::with_capacity(capacity),
            meshes: Vec::with_capacity(capacity),
            transforms: Vec::with_capacity(capacity),
            previous_transforms: Vec::with_capacity(capacity),
            materials: Vec::with_capacity(capacity),
            layers: Vec::with_capacity(capacity),
            casts_shadows: Vec::with_capacity(capacity),
            visible: Vec::with_capacity(capacity),
            centers: Vec::with_capacity(capacity),
            radii: Vec::with_capacity(capacity),
            records: Vec::with_capacity(capacity),
            previous_records: Vec::with_capacity(capacity),
            dirty_flags: Vec::with_capacity(capacity),
            moved_flags: Vec::with_capacity(capacity),
            mesh_radii: Vec::with_capacity(16),
            material_ids: Vec::new(),
            free: Vec::with_capacity(capacity),
            dirty: Vec::with_capacity(capacity),
            previous_dirty: Vec::with_capacity(capacity),
            moved: Vec::with_capacity(capacity),
            alive: 0,
        }
    }

    /// Adds a primitive for an uploaded mesh.
    ///
    /// # Errors
    /// Returns [`RenderError::UnknownMesh`] when `mesh` is not registered with `Gfx`.
    pub fn add(&mut self, mesh: MeshId, desc: PrimitiveDesc) -> Result<PrimitiveId, RenderError> {
        self.validate_material(desc.material)?;
        let mesh_index =
            usize::try_from(mesh.index()).map_err(|_| RenderError::UnknownMesh(mesh))?;
        let mesh_radius = self
            .mesh_radii
            .get(mesh_index)
            .copied()
            .flatten()
            .filter(|(registered, _)| *registered == mesh)
            .map(|(_, radius)| radius)
            .ok_or(RenderError::UnknownMesh(mesh))?;
        let index = if let Some(index) = self.free.pop() {
            index
        } else {
            if self.len_slots() == self.generations.capacity() {
                self.grow();
            }
            u32::try_from(self.len_slots()).map_err(|_| RenderError::SceneCapacity)?
        };
        let slot = usize::try_from(index).map_err(|_| RenderError::SceneCapacity)?;
        if slot == self.len_slots() {
            self.push_default();
        }
        let generation = *self
            .generations
            .get(slot)
            .ok_or(RenderError::SceneCapacity)?;
        let (
            Some(live),
            Some(meshes),
            Some(transforms),
            Some(previous_transforms),
            Some(materials),
            Some(layers),
            Some(casts_shadows),
            Some(visible),
            Some(records),
            Some(previous_records),
        ) = (
            self.live.get_mut(slot),
            self.meshes.get_mut(slot),
            self.transforms.get_mut(slot),
            self.previous_transforms.get_mut(slot),
            self.materials.get_mut(slot),
            self.layers.get_mut(slot),
            self.casts_shadows.get_mut(slot),
            self.visible.get_mut(slot),
            self.records.get_mut(slot),
            self.previous_records.get_mut(slot),
        )
        else {
            return Err(RenderError::SceneCapacity);
        };
        *live = true;
        *meshes = mesh;
        *transforms = desc.transform;
        *previous_transforms = desc.transform;
        *materials = desc.material;
        *layers = desc.layer;
        *casts_shadows = desc.casts_shadow;
        *visible = true;
        *records = encode_instance(
            desc.transform,
            look_for_material(desc.material),
            mesh.index(),
        );
        *previous_records = PreviousTransform::from_mat4(Mat4::from(desc.transform));
        self.update_bounds(slot, mesh_radius);
        self.previous_dirty.push(index);
        self.alive = self.alive.saturating_add(1);
        self.mark_dirty(index)?;
        Ok(PrimitiveId { index, generation })
    }

    /// Changes a primitive transform.
    ///
    /// # Errors
    /// Returns [`RenderError::StalePrimitive`] when `id` is no longer live.
    pub fn set_transform(
        &mut self,
        id: PrimitiveId,
        transform: Affine3A,
    ) -> Result<(), RenderError> {
        let slot = self.slot_index(id)?;
        let current = *self
            .transforms
            .get(slot)
            .ok_or(RenderError::SceneCapacity)?;
        if current.to_cols_array().map(f32::to_bits) == transform.to_cols_array().map(f32::to_bits)
        {
            return Ok(());
        }
        let material = *self.materials.get(slot).ok_or(RenderError::SceneCapacity)?;
        let mesh = *self.meshes.get(slot).ok_or(RenderError::SceneCapacity)?;
        let mesh_radius = self.mesh_radius(mesh)?;
        let should_mark_moved = !*self
            .moved_flags
            .get(slot)
            .ok_or(RenderError::SceneCapacity)?;
        let record = self
            .records
            .get_mut(slot)
            .ok_or(RenderError::SceneCapacity)?;
        *self
            .transforms
            .get_mut(slot)
            .ok_or(RenderError::SceneCapacity)? = transform;
        *record = encode_instance(transform, look_for_material(material), mesh.index());
        self.update_bounds(slot, mesh_radius);
        if should_mark_moved {
            *self
                .moved_flags
                .get_mut(slot)
                .ok_or(RenderError::SceneCapacity)? = true;
            self.moved.push(id.index);
        }
        self.mark_dirty(id.index)
    }

    /// Changes a primitive look.
    ///
    /// # Errors
    /// Returns [`RenderError::StalePrimitive`] when `id` is no longer live.
    pub fn set_look(&mut self, id: PrimitiveId, look: Look) -> Result<(), RenderError> {
        self.set_material(id, Material::Procedural(look))
    }

    /// Changes a primitive's material source.
    ///
    /// # Errors
    /// Returns [`RenderError::StalePrimitive`] for an invalid primitive and
    /// [`RenderError::UnknownMaterial`] for a stale textured material handle.
    pub fn set_material(&mut self, id: PrimitiveId, material: Material) -> Result<(), RenderError> {
        self.validate_material(material)?;
        let slot = self.slot_index(id)?;
        let transform = *self
            .transforms
            .get(slot)
            .ok_or(RenderError::SceneCapacity)?;
        let mesh = *self.meshes.get(slot).ok_or(RenderError::SceneCapacity)?;
        *self
            .materials
            .get_mut(slot)
            .ok_or(RenderError::SceneCapacity)? = material;
        *self
            .records
            .get_mut(slot)
            .ok_or(RenderError::SceneCapacity)? =
            encode_instance(transform, look_for_material(material), mesh.index());
        self.mark_dirty(id.index)
    }

    /// Re-points every live primitive using `old` to the replacement mesh at a frame boundary.
    ///
    /// # Errors
    /// Returns [`RenderError::UnknownMesh`] if `new` is not registered.
    pub fn replace_mesh(&mut self, old: MeshId, new: MeshId) -> Result<(), RenderError> {
        let new_radius = self.mesh_radius(new)?;
        for slot in 0..self.len_slots() {
            if !self.live.get(slot).copied().unwrap_or(false)
                || self.meshes.get(slot).copied() != Some(old)
            {
                continue;
            }
            let Some(mesh) = self.meshes.get_mut(slot) else {
                continue;
            };
            *mesh = new;
            let Some(transform) = self.transforms.get(slot).copied() else {
                continue;
            };
            let Some(material) = self.materials.get(slot).copied() else {
                continue;
            };
            if let Some(record) = self.records.get_mut(slot) {
                *record = encode_instance(transform, look_for_material(material), new.index());
            }
            self.update_bounds(slot, new_radius);
            if let Ok(index) = u32::try_from(slot) {
                self.mark_dirty(index)?;
            }
        }
        Ok(())
    }

    /// Re-points every live primitive using `old` to the replacement material at a frame boundary.
    ///
    /// # Errors
    /// Returns [`RenderError::UnknownMaterial`] if `new` is not registered.
    pub fn replace_material(
        &mut self,
        old: MaterialId,
        new: MaterialId,
    ) -> Result<(), RenderError> {
        self.validate_material(Material::Textured(new))?;
        for slot in 0..self.len_slots() {
            let Some(material) = self.materials.get(slot).copied() else {
                continue;
            };
            if !self.live.get(slot).copied().unwrap_or(false) || material != Material::Textured(old)
            {
                continue;
            }
            if let Some(place) = self.materials.get_mut(slot) {
                *place = Material::Textured(new);
            }
            if let Ok(index) = u32::try_from(slot) {
                self.mark_dirty(index)?;
            }
        }
        Ok(())
    }

    pub(crate) fn register_material(&mut self, material: MaterialId) {
        let Ok(index) = usize::try_from(material.index()) else {
            return;
        };
        while self.material_ids.len() <= index {
            self.material_ids.push(None);
        }
        if let Some(slot) = self.material_ids.get_mut(index) {
            *slot = Some(material);
        }
    }

    pub(crate) fn unregister_material(&mut self, material: MaterialId) -> Result<(), RenderError> {
        let index = usize::try_from(material.index())
            .map_err(|_| RenderError::UnknownMaterial(material))?;
        let registered = self
            .material_ids
            .get_mut(index)
            .ok_or(RenderError::UnknownMaterial(material))?;
        if *registered != Some(material) {
            return Err(RenderError::UnknownMaterial(material));
        }
        *registered = None;
        Ok(())
    }

    fn validate_material(&self, material: Material) -> Result<(), RenderError> {
        if let Material::Textured(id) = material {
            let index =
                usize::try_from(id.index()).map_err(|_| RenderError::UnknownMaterial(id))?;
            if self.material_ids.get(index).copied().flatten() != Some(id) {
                return Err(RenderError::UnknownMaterial(id));
            }
        }
        Ok(())
    }

    /// Changes a primitive's visibility.
    ///
    /// # Errors
    /// Returns [`RenderError::StalePrimitive`] when `id` is no longer live.
    pub fn set_visible(&mut self, id: PrimitiveId, visible: bool) -> Result<(), RenderError> {
        let slot = self.slot_index(id)?;
        *self
            .visible
            .get_mut(slot)
            .ok_or(RenderError::SceneCapacity)? = visible;
        self.mark_dirty(id.index)
    }

    /// Removes a primitive and releases its slot for generation-bumped reuse.
    ///
    /// # Errors
    /// Returns [`RenderError::StalePrimitive`] when `id` is no longer live.
    pub fn remove(&mut self, id: PrimitiveId) -> Result<(), RenderError> {
        let slot = self.slot_index(id)?;
        let generation = self
            .generations
            .get(slot)
            .copied()
            .ok_or(RenderError::SceneCapacity)?;
        let next_generation = generation
            .checked_add(1)
            .ok_or(RenderError::SceneCapacity)?;
        *self.live.get_mut(slot).ok_or(RenderError::SceneCapacity)? = false;
        *self
            .visible
            .get_mut(slot)
            .ok_or(RenderError::SceneCapacity)? = false;
        *self
            .dirty_flags
            .get_mut(slot)
            .ok_or(RenderError::SceneCapacity)? = false;
        *self
            .generations
            .get_mut(slot)
            .ok_or(RenderError::SceneCapacity)? = next_generation;
        self.alive = self.alive.saturating_sub(1);
        self.mark_dirty(id.index)?;
        self.free.push(id.index);
        Ok(())
    }

    /// Number of allocated live primitives.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.alive
    }
    /// Whether the scene has no live primitives.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.alive == 0
    }
    /// Number of allocated slots, including free slots.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.generations.capacity()
    }

    pub(crate) fn len_slots(&self) -> usize {
        self.generations.len()
    }
    pub(crate) fn slot(&self, index: usize) -> Option<SceneSlot> {
        Some(SceneSlot {
            live: *self.live.get(index)?,
            mesh: *self.meshes.get(index)?,
            material: *self.materials.get(index)?,
            layer: *self.layers.get(index)?,
            casts_shadow: *self.casts_shadows.get(index)?,
            visible: *self.visible.get(index)?,
            center: *self.centers.get(index)?,
            radius: *self.radii.get(index)?,
        })
    }
    pub(crate) fn record_range(&self, range: std::ops::Range<usize>) -> Option<&[Instance]> {
        self.records.get(range)
    }
    pub(crate) fn previous_record_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> Option<&[PreviousTransform]> {
        self.previous_records.get(range)
    }
    pub(crate) fn dirty_slots(&self) -> &[u32] {
        &self.dirty
    }
    pub(crate) fn build_dirty_ranges(&mut self, out: &mut Vec<(u32, u32)>) {
        dirty_ranges(&mut self.dirty, out);
    }
    pub(crate) fn build_previous_dirty_ranges(&mut self, out: &mut Vec<(u32, u32)>) {
        dirty_ranges(&mut self.previous_dirty, out);
    }
    pub(crate) fn clear_dirty(&mut self) {
        for &index in &self.dirty {
            if let Ok(slot) = usize::try_from(index)
                && let Some(flag) = self.dirty_flags.get_mut(slot)
            {
                *flag = false;
            }
        }
        self.dirty.clear();
        self.previous_dirty.clear();
    }
    pub(crate) fn register_mesh(&mut self, mesh: MeshId, radius: f32) {
        let Ok(index) = usize::try_from(mesh.index()) else {
            return;
        };
        while self.mesh_radii.len() <= index {
            self.mesh_radii.push(None);
        }
        if let Some(slot) = self.mesh_radii.get_mut(index) {
            *slot = Some((mesh, radius));
        }
    }
    pub(crate) fn unregister_mesh(&mut self, mesh: MeshId) -> Result<(), RenderError> {
        let index = usize::try_from(mesh.index()).map_err(|_| RenderError::UnknownMesh(mesh))?;
        let slot = self
            .mesh_radii
            .get_mut(index)
            .ok_or(RenderError::UnknownMesh(mesh))?;
        if slot.is_none_or(|(registered, _)| registered != mesh) {
            return Err(RenderError::UnknownMesh(mesh));
        }
        *slot = None;
        Ok(())
    }
    pub(crate) fn advance_frame(&mut self) {
        for index in self.moved.drain(..) {
            if let Ok(slot) = usize::try_from(index) {
                if let (Some(previous), Some(current)) = (
                    self.previous_records.get_mut(slot),
                    self.transforms.get(slot),
                ) {
                    *previous = PreviousTransform::from_mat4(Mat4::from(*current));
                }
                if let Some(previous_transform) = self.previous_transforms.get_mut(slot)
                    && let Some(current) = self.transforms.get(slot)
                {
                    *previous_transform = *current;
                }
                if let Some(flag) = self.moved_flags.get_mut(slot) {
                    *flag = false;
                }
                if !self.previous_dirty.contains(&index) {
                    self.previous_dirty.push(index);
                }
            }
        }
    }
    /// Culls the dense scene and reports each visible primitive to `emit` without allocating.
    ///
    /// `camera_planes` controls world and glow visibility; `cascade_planes` independently
    /// controls world shadow visibility. View-model primitives bypass the world frustum.
    pub fn cull(
        &self,
        camera_planes: &[Vec4; 6],
        cascade_planes: &[[Vec4; 6]; 2],
        mut emit: impl FnMut(MeshId, Material, u32, Layer, bool, bool, [bool; 2], Vec3, f32),
    ) -> CullStats {
        let mut stats = CullStats::default();
        let mut shadow_counts = [0u32; 2];
        let shadow_capacity = u32::try_from(MAX_INSTANCES).unwrap_or(u32::MAX);
        for slot_index in 0..self.len_slots() {
            let Some(slot) = self.slot(slot_index) else {
                continue;
            };
            if !slot.live || !slot.visible {
                continue;
            }
            let Ok(slot_id) = u32::try_from(slot_index) else {
                continue;
            };
            match slot.layer {
                Layer::ViewModel => emit(
                    slot.mesh,
                    slot.material,
                    slot_id,
                    slot.layer,
                    true,
                    false,
                    [false; 2],
                    slot.center,
                    slot.radius,
                ),
                Layer::Glow => {
                    if sphere_visible_in_planes(camera_planes, slot.center, slot.radius) {
                        emit(
                            slot.mesh,
                            slot.material,
                            slot_id,
                            slot.layer,
                            true,
                            false,
                            [false; 2],
                            slot.center,
                            slot.radius,
                        );
                    }
                }
                Layer::World => {
                    let visible = sphere_visible_in_planes(camera_planes, slot.center, slot.radius);
                    let shadow_visible = std::array::from_fn(|cascade| {
                        let allowed = slot.casts_shadow
                            && shadow_counts
                                .get(cascade)
                                .copied()
                                .unwrap_or(shadow_capacity)
                                < shadow_capacity;
                        allowed
                            && cascade_planes.get(cascade).is_some_and(|planes| {
                                sphere_visible_in_planes(planes, slot.center, slot.radius)
                            })
                    });
                    if visible {
                        stats.visible_main = stats.visible_main.saturating_add(1);
                    } else {
                        stats.culled = stats.culled.saturating_add(1);
                    }
                    for (cascade, is_visible) in shadow_visible.into_iter().enumerate() {
                        if is_visible {
                            if let Some(count) = shadow_counts.get_mut(cascade) {
                                *count = count.saturating_add(1);
                            }
                            if let Some(count) = stats.visible_shadow.get_mut(cascade) {
                                *count = count.saturating_add(1);
                            }
                        }
                    }
                    if visible || shadow_visible.into_iter().any(|is_visible| is_visible) {
                        emit(
                            slot.mesh,
                            slot.material,
                            slot_id,
                            slot.layer,
                            visible,
                            slot.casts_shadow,
                            shadow_visible,
                            slot.center,
                            slot.radius,
                        );
                    }
                }
            }
        }
        stats
    }

    pub(crate) fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    pub(crate) fn uses_mesh(&self, mesh: MeshId) -> bool {
        self.live
            .iter()
            .zip(self.meshes.iter())
            .any(|(live, used_mesh)| *live && *used_mesh == mesh)
    }

    pub(crate) fn uses_material(&self, material: MaterialId) -> bool {
        self.live
            .iter()
            .zip(self.materials.iter())
            .any(|(live, used)| *live && matches!(used, Material::Textured(id) if *id == material))
    }

    fn grow(&mut self) {
        let target = self.generations.capacity().max(1).saturating_mul(2);
        macro_rules! reserve_to { ($($v:expr),+ $(,)?) => { $($v.reserve_exact(target.saturating_sub($v.capacity()));)+ }; }
        reserve_to!(
            self.generations,
            self.live,
            self.meshes,
            self.transforms,
            self.previous_transforms,
            self.materials,
            self.layers,
            self.casts_shadows,
            self.visible,
            self.centers,
            self.radii,
            self.records,
            self.previous_records,
            self.dirty_flags,
            self.moved_flags,
            self.free,
            self.dirty,
            self.previous_dirty,
            self.moved
        );
    }
    fn push_default(&mut self) {
        self.generations.push(0);
        self.live.push(false);
        self.meshes.push(MeshId::procedural(0));
        self.transforms.push(Affine3A::IDENTITY);
        self.previous_transforms.push(Affine3A::IDENTITY);
        self.materials.push(Material::Procedural(Look::default()));
        self.layers.push(Layer::World);
        self.casts_shadows.push(true);
        self.visible.push(false);
        self.centers.push(Vec3::ZERO);
        self.radii.push(0.0);
        self.records.push(Instance::new(Mat4::IDENTITY));
        self.previous_records
            .push(PreviousTransform::from_mat4(Mat4::IDENTITY));
        self.dirty_flags.push(false);
        self.moved_flags.push(false);
    }
    fn update_bounds(&mut self, slot: usize, mesh_radius: f32) {
        if let (Some(transform), Some(center), Some(radius)) = (
            self.transforms.get(slot),
            self.centers.get_mut(slot),
            self.radii.get_mut(slot),
        ) {
            *center = transform.translation.into();
            let matrix = transform.matrix3;
            let scale = matrix
                .x_axis
                .length()
                .max(matrix.y_axis.length())
                .max(matrix.z_axis.length());
            *radius = mesh_radius * scale;
        }
    }
    fn mesh_radius(&self, mesh: MeshId) -> Result<f32, RenderError> {
        usize::try_from(mesh.index())
            .ok()
            .and_then(|index| self.mesh_radii.get(index).copied().flatten())
            .filter(|(registered, _)| *registered == mesh)
            .map(|(_, radius)| radius)
            .ok_or(RenderError::UnknownMesh(mesh))
    }
    fn slot_index(&self, id: PrimitiveId) -> Result<usize, RenderError> {
        let index = usize::try_from(id.index).map_err(|_| RenderError::StalePrimitive(id))?;
        match (self.live.get(index), self.generations.get(index)) {
            (Some(true), Some(&generation)) if generation == id.generation => Ok(index),
            _ => Err(RenderError::StalePrimitive(id)),
        }
    }
    fn mark_dirty(&mut self, index: u32) -> Result<(), RenderError> {
        let slot = usize::try_from(index).map_err(|_| RenderError::SceneCapacity)?;
        let flag = self
            .dirty_flags
            .get_mut(slot)
            .ok_or(RenderError::SceneCapacity)?;
        if !*flag {
            *flag = true;
            self.dirty.push(index);
        }
        Ok(())
    }
}

fn look_for_material(material: Material) -> Look {
    match material {
        Material::Procedural(look) => look,
        Material::Textured(_) => Look::default(),
    }
}

fn encode_instance(transform: Affine3A, look: Look, mesh_index: u32) -> Instance {
    let mut instance = Instance::new(Mat4::from(transform))
        .tint([look.tint[0], look.tint[1], look.tint[2]])
        .emissive(look.emissive);
    if let Some(value) = look.roughness {
        instance = instance.rough(value);
    }
    if let Some(value) = look.metalness {
        instance = instance.metal(value);
    }
    if let Some(pattern) = look.pattern {
        instance = instance.pattern(pattern);
    }
    let flags = (u32::from(look.object_space_pattern) * FLAG_OBJECT_SPACE)
        | (u32::from(look.unlit) * FLAG_UNLIT);
    instance.flags(flags).mesh_index(mesh_index)
}

/// Coalesces sorted dirty slot indices into half-open GPU write ranges.
pub(crate) fn dirty_ranges(dirty: &mut [u32], out: &mut Vec<(u32, u32)>) {
    dirty.sort_unstable();
    out.clear();
    for &index in dirty.iter() {
        match out.last_mut() {
            Some((_, end)) if *end == index => *end = index.saturating_add(1),
            _ => out.push((index, index.saturating_add(1))),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn desc(x: f32) -> PrimitiveDesc {
        PrimitiveDesc {
            transform: Affine3A::from_translation(Vec3::new(x, 0.0, 0.0)),
            material: Material::Procedural(Look::default()),
            layer: Layer::World,
            casts_shadow: true,
        }
    }
    fn scene() -> RenderScene {
        let mut scene = RenderScene::with_capacity(1);
        scene.register_mesh(MeshId::procedural(0), 1.0);
        scene
    }

    #[test]
    fn add_update_and_remove_keep_slot_state_consistent() {
        let mut scene = scene();
        let id = scene.add(MeshId::procedural(0), desc(1.0)).expect("add");
        scene.clear_dirty();
        scene
            .set_transform(id, Affine3A::from_translation(Vec3::X * 4.0))
            .expect("transform");
        scene.set_visible(id, false).expect("visibility");
        let mut ranges = Vec::with_capacity(1);
        dirty_ranges(&mut scene.dirty, &mut ranges);
        assert_eq!(ranges, [(id.index, id.index.saturating_add(1))]);
        let slot = scene.slot(0).expect("slot");
        assert_eq!(
            scene.transforms.first().expect("transform").translation.x,
            4.0
        );
        assert!(!slot.visible);
        scene.remove(id).expect("remove");
        assert!(scene.is_empty());
        assert!(matches!(
            scene.set_visible(id, true),
            Err(RenderError::StalePrimitive(_))
        ));
    }
    #[test]
    fn reused_slot_increments_its_generation() {
        let mut scene = scene();
        let first = scene.add(MeshId::procedural(0), desc(0.0)).expect("first");
        scene.remove(first).expect("remove");
        let second = scene.add(MeshId::procedural(0), desc(1.0)).expect("second");
        assert_eq!(first.index, second.index);
        assert_eq!(first.generation + 1, second.generation);
        assert!(matches!(
            scene.remove(first),
            Err(RenderError::StalePrimitive(_))
        ));
    }
    #[test]
    fn capacity_grows_and_existing_slots_survive() {
        let mut scene = scene();
        let first = scene.add(MeshId::procedural(0), desc(1.0)).expect("first");
        let second = scene.add(MeshId::procedural(0), desc(2.0)).expect("second");
        assert!(scene.capacity() >= 2);
        assert_eq!(
            scene
                .transforms
                .get(usize::try_from(first.index).expect("index fits"))
                .map(|transform| transform.translation.x),
            Some(1.0)
        );
        assert_eq!(
            scene
                .transforms
                .get(usize::try_from(second.index).expect("index fits"))
                .map(|transform| transform.translation.x),
            Some(2.0)
        );
    }
    #[test]
    fn adjacent_dirty_slots_coalesce_into_one_write_range() {
        let mut dirty = [4, 2, 3, 8, 10, 9];
        let mut ranges = Vec::with_capacity(3);
        dirty_ranges(&mut dirty, &mut ranges);
        assert_eq!(ranges, [(2, 5), (8, 11)]);
    }

    proptest! {
        #[test]
        fn random_scene_updates_match_brute_force_visibility(
            actions in prop::collection::vec((0u8..4, -40i16..40, 0u8..2, 0u8..2), 1..80),
            camera_x in -10i16..10,
            camera_yaw in -100i16..100,
        ) {
            use crate::gfx::frustum_planes;
            use glam::{Mat4, Vec3};

            fn brute(planes: &[Vec4; 6], center: Vec3, radius: f32) -> bool {
                planes.iter().all(|plane| plane.truncate().dot(center) + plane.w > -radius)
            }

            let mut scene = RenderScene::with_capacity(16);
            scene.register_mesh(MeshId::procedural(0), 1.0);
            scene.register_mesh(MeshId::procedural(1), 1.5);
            let mut ids = Vec::<PrimitiveId>::new();
            let mut reference = Vec::<Option<(Affine3A, bool, bool, MeshId, f32)>>::new();
            for (action, x, visibility, casts) in actions {
                match action {
                    0 if ids.is_empty() || reference.iter().all(Option::is_none) => {
                        let transform = Affine3A::from_translation(Vec3::new(f32::from(x), 0.0, -8.0));
                        let mut desc = desc(f32::from(x));
                        desc.transform = transform;
                        desc.casts_shadow = casts != 0;
                        let mesh = MeshId::procedural(u32::from(visibility));
                        let id = scene.add(mesh, desc).expect("add");
                        let slot = usize::try_from(id.index).expect("index fits");
                        if reference.len() <= slot { reference.resize(slot + 1, None); }
                        let radius = if mesh.index() == 0 { 1.0 } else { 1.5 };
                        *reference.get_mut(slot).expect("reference slot") =
                            Some((transform, true, casts != 0, mesh, radius));
                        ids.push(id);
                    }
                    1 if !ids.is_empty() => {
                        let id = *ids
                            .get(usize::from(x.unsigned_abs()) % ids.len())
                            .expect("selected id");
                        if let Ok(slot) = scene.slot_index(id) {
                            let transform = Affine3A::from_translation(Vec3::new(f32::from(x), 0.0, -8.0));
                            scene.set_transform(id, transform).expect("move");
                            if let Some(entry) = reference.get_mut(slot).and_then(Option::as_mut) { entry.0 = transform; }
                        }
                    }
                    2 if !ids.is_empty() => {
                        let id = *ids
                            .get(usize::from(x.unsigned_abs()) % ids.len())
                            .expect("selected id");
                        if let Ok(slot) = scene.slot_index(id) {
                            scene.set_visible(id, visibility != 0).expect("visibility");
                            if let Some(entry) = reference.get_mut(slot).and_then(Option::as_mut) { entry.1 = visibility != 0; }
                        }
                    }
                    3 if !ids.is_empty() => {
                        let index = usize::from(x.unsigned_abs()) % ids.len();
                        let id = ids.remove(index);
                        if let Ok(slot) = scene.slot_index(id) {
                            scene.remove(id).expect("remove");
                            if let Some(entry) = reference.get_mut(slot) { *entry = None; }
                        }
                    }
                    _ => {}
                }

                let view = Mat4::from_rotation_y(f32::from(camera_yaw) * 0.01)
                    * Mat4::from_translation(Vec3::new(-f32::from(camera_x), 0.0, 0.0));
                let planes = frustum_planes(&(Mat4::orthographic_rh(-20.0, 20.0, -20.0, 20.0, 0.1, 40.0) * view));
                let cascade_planes = [
                    frustum_planes(&(Mat4::orthographic_rh(-8.0, 8.0, -8.0, 8.0, 0.1, 20.0) * view)),
                    frustum_planes(&(Mat4::orthographic_rh(-30.0, 30.0, -30.0, 30.0, 0.1, 60.0) * view)),
                ];
                let mut actual_main = Vec::new();
                let mut actual_shadow = [Vec::new(), Vec::new()];
                scene.cull(&planes, &cascade_planes, |mesh, _material, slot, layer, main, _, shadows, _, _| {
                    if layer == Layer::World {
                        if main { actual_main.push((mesh, slot)); }
                        for (cascade, visible) in shadows.into_iter().enumerate() {
                            if visible
                                && let Some(indices) = actual_shadow.get_mut(cascade)
                            {
                                indices.push((mesh, slot));
                            }
                        }
                    }
                });
                let mut expected_main = Vec::new();
                let mut expected_shadow = [Vec::new(), Vec::new()];
                for (slot_index, entry) in reference.iter().enumerate() {
                    if let Some((transform, visible, casts_shadow, mesh, radius)) = entry {
                        let center = transform.translation.into();
                        let slot_id = u32::try_from(slot_index).expect("slot fits");
                        if *visible && brute(&planes, center, *radius) {
                            expected_main.push((*mesh, slot_id));
                        }
                        if *visible && *casts_shadow {
                            for cascade in 0..2 {
                                if let Some(planes) = cascade_planes.get(cascade)
                                    && brute(planes, center, *radius)
                                    && let Some(indices) = expected_shadow.get_mut(cascade)
                                {
                                    indices.push((*mesh, slot_id));
                                }
                            }
                        }
                    }
                }
                actual_main.sort_unstable_by_key(|(mesh, slot)| (mesh.index(), *slot));
                expected_main.sort_unstable_by_key(|(mesh, slot)| (mesh.index(), *slot));
                for cascade in 0..2 {
                    if let Some(indices) = actual_shadow.get_mut(cascade) {
                        indices.sort_unstable_by_key(|(mesh, slot)| (mesh.index(), *slot));
                    }
                    if let Some(indices) = expected_shadow.get_mut(cascade) {
                        indices.sort_unstable_by_key(|(mesh, slot)| (mesh.index(), *slot));
                    }
                }
                prop_assert_eq!(actual_main, expected_main);
                prop_assert_eq!(actual_shadow, expected_shadow);
            }
        }
    }
}
