//! Parent-first scene nodes and hierarchy validation.
//!
//! Node transforms are finite, and parents must precede children for simple iterative traversal.
use serde::{Deserialize, Serialize};

use super::MeshPack;

/// One node with parent-before-child order and rest transform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Source node name, if present.
    pub name: String,
    /// Parent node index; parents precede children.
    pub parent: Option<u32>,
    /// Rest translation.
    pub translation: [f32; 3],
    /// Rest rotation as x, y, z, w.
    pub rotation: [f32; 4],
    /// Rest scale.
    pub scale: [f32; 3],
    /// Blender custom properties, preserved as strings.
    pub tags: Vec<(String, String)>,
}

pub(super) fn is_valid_tree(pack: &MeshPack) -> bool {
    pack.nodes.iter().enumerate().all(|(index, node)| {
        node.translation.iter().all(|value| value.is_finite())
            && node.rotation.iter().all(|value| value.is_finite())
            && node.scale.iter().all(|value| value.is_finite())
            && node.parent.is_none_or(|parent| {
                usize::try_from(parent).is_ok_and(|parent_index| parent_index < index)
            })
    })
}
