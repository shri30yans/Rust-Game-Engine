//! The only way to create a [`Structure`]. Every rule is checked here, so a
//! structure that exists is always valid: finite positive physics, no self or
//! duplicate edges, at least one anchor, and every element connected to one.
//!
//! Invariant: element and material ids are issued in insertion order and are
//! never reused, so an id that passed a check stays valid for the builder's life.

use crate::element::{ElementDef, ElementSpec};
use crate::error::{DestructionError, at, at_mut, require_finite_point, require_positive};
use crate::ids::{ElementId, MaterialId};
use crate::material::Material;
use crate::structure::{DestructionConfig, Structure};

/// Collects materials, elements and edges, then validates them into a [`Structure`].
#[derive(Debug, Clone)]
pub struct StructureBuilder {
    config: DestructionConfig,
    materials: Vec<Material>,
    specs: Vec<ElementSpec>,
    edges: Vec<(ElementId, ElementId)>,
}

impl StructureBuilder {
    /// Starts an empty builder with the given rules.
    #[must_use]
    pub fn new(config: DestructionConfig) -> Self {
        Self {
            config,
            materials: Vec::new(),
            specs: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Registers a material and returns its id.
    ///
    /// # Errors
    /// Returns [`DestructionError::TooManyMaterials`] when the id space is full.
    pub fn add_material(&mut self, material: Material) -> Result<MaterialId, DestructionError> {
        let id = MaterialId::from_index(self.materials.len())
            .ok_or(DestructionError::TooManyMaterials)?;
        self.materials.push(material);
        Ok(id)
    }

    /// Adds an element and returns its id.
    ///
    /// # Errors
    /// Fails when the material is unknown, volume or area is not finite and
    /// positive, the position is not finite, or the id space is full.
    pub fn add_element(&mut self, spec: ElementSpec) -> Result<ElementId, DestructionError> {
        self.material(spec.material)?;
        require_positive("volume", spec.volume)?;
        require_positive("area", spec.area)?;
        require_finite_point("position", spec.position)?;
        let id =
            ElementId::from_index(self.specs.len()).ok_or(DestructionError::TooManyElements)?;
        self.specs.push(spec);
        Ok(id)
    }

    /// Connects two elements. Edges are undirected; duplicates are rejected at build time.
    ///
    /// # Errors
    /// Fails for a self edge or an element id this builder did not issue.
    pub fn connect(&mut self, a: ElementId, b: ElementId) -> Result<(), DestructionError> {
        if a == b {
            return Err(DestructionError::SelfEdge(a));
        }
        self.require_element(a)?;
        self.require_element(b)?;
        self.edges.push(if a < b { (a, b) } else { (b, a) });
        Ok(())
    }

    /// Validates everything and produces an immutable [`Structure`].
    ///
    /// # Errors
    /// Fails for an empty builder, no anchor, a duplicate edge, a non-finite
    /// derived mass or capacity, or an element with no path to an anchor.
    pub fn build(self) -> Result<Structure, DestructionError> {
        if self.specs.is_empty() {
            return Err(DestructionError::Empty);
        }
        let anchors: Vec<ElementId> = self
            .specs
            .iter()
            .enumerate()
            .filter(|(_, spec)| spec.anchor)
            .filter_map(|(index, _)| ElementId::from_index(index))
            .collect();
        if anchors.is_empty() {
            return Err(DestructionError::NoAnchor);
        }
        let defs = self.element_defs()?;
        let (offsets, adjacency) = self.adjacency()?;
        let structure = Structure::new(defs, anchors, offsets, adjacency, self.config);
        if let Some(id) = structure.first_unanchored()? {
            return Err(DestructionError::Unanchored(id));
        }
        Ok(structure)
    }

    fn material(&self, id: MaterialId) -> Result<Material, DestructionError> {
        self.materials
            .get(id.index())
            .copied()
            .ok_or(DestructionError::UnknownMaterial(id))
    }

    fn require_element(&self, id: ElementId) -> Result<(), DestructionError> {
        if id.index() < self.specs.len() {
            Ok(())
        } else {
            Err(DestructionError::UnknownElement(id))
        }
    }

    fn element_defs(&self) -> Result<Vec<ElementDef>, DestructionError> {
        self.specs
            .iter()
            .map(|spec| {
                let material = self.material(spec.material)?;
                let mass = require_positive("mass", material.density() * spec.volume)?;
                let capacity =
                    require_positive("capacity", material.strength_per_area() * spec.area)?;
                Ok(ElementDef::new(
                    mass,
                    capacity,
                    spec.max_health,
                    spec.anchor,
                    spec.position,
                ))
            })
            .collect()
    }

    /// Builds the compressed adjacency, rejecting duplicate edges. Neighbour
    /// lists come out sorted by id.
    fn adjacency(&self) -> Result<(Vec<usize>, Vec<ElementId>), DestructionError> {
        let n = self.specs.len();
        let mut edges = self.edges.clone();
        edges.sort_unstable();
        for pair in edges.windows(2) {
            if let [first, second] = pair
                && first == second
            {
                return Err(DestructionError::DuplicateEdge(first.0, first.1));
            }
        }

        let mut degree = vec![0usize; n];
        for &(a, b) in &edges {
            *at_mut(&mut degree, a.index())? += 1;
            *at_mut(&mut degree, b.index())? += 1;
        }
        let mut offsets = Vec::with_capacity(n + 1);
        offsets.push(0usize);
        let mut total = 0usize;
        for &count in &degree {
            total = total
                .checked_add(count)
                .ok_or(DestructionError::Invariant("adjacency size overflow"))?;
            offsets.push(total);
        }

        let mut cursor = offsets.clone();
        let mut adjacency = vec![ElementId::new(0); total];
        for &(a, b) in &edges {
            place(&mut cursor, &mut adjacency, a, b)?;
            place(&mut cursor, &mut adjacency, b, a)?;
        }
        for index in 0..n {
            let start = Structure::offset(&offsets, index)?;
            let end = Structure::offset(&offsets, index + 1)?;
            adjacency
                .get_mut(start..end)
                .ok_or(DestructionError::Invariant("adjacency range out of bounds"))?
                .sort_unstable();
        }
        Ok((offsets, adjacency))
    }
}

/// Writes `to` into the next free adjacency slot of `from`.
fn place(
    cursor: &mut [usize],
    adjacency: &mut [ElementId],
    from: ElementId,
    to: ElementId,
) -> Result<(), DestructionError> {
    let slot_index = at(cursor, from.index())?;
    let slot = at_mut(adjacency, *slot_index)?;
    *slot = to;
    *at_mut(cursor, from.index())? += 1;
    Ok(())
}
