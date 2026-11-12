//! Cook-time convex decomposition of triangle meshes into bounded convex hull point clouds.
//!
//! `DecompositionParams` validates the cooker budget once, when it is built. The hull count of a
//! successful decomposition never exceeds `max_hulls`.

use super::convex::ConvexShape;
use crate::PhysicsError;
use glam::Vec3;
use rapier3d::prelude::SharedShape;

/// Bounded, deterministic settings for cook-time convex decomposition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecompositionParams {
    /// VHACD voxel resolution.
    resolution: u32,
    /// Maximum convex hull count.
    max_hulls: u32,
    /// Maximum concavity threshold.
    concavity: f32,
}
impl DecompositionParams {
    /// Builds settings within the supported cooker budget.
    pub fn new(resolution: u32, max_hulls: u32, concavity: f32) -> Result<Self, PhysicsError> {
        if !(16..=256).contains(&resolution)
            || !(1..=64).contains(&max_hulls)
            || !concavity.is_finite()
            || concavity <= 0.0
            || concavity > 0.1
        {
            return Err(PhysicsError::InvalidDecompositionParams);
        }
        Ok(Self {
            resolution,
            max_hulls,
            concavity,
        })
    }

    /// Default deterministic VHACD settings.
    pub fn default_settings() -> Self {
        Self {
            resolution: 64,
            max_hulls: 16,
            concavity: 0.0025,
        }
    }
}
impl Default for DecompositionParams {
    fn default() -> Self {
        Self {
            resolution: 64,
            max_hulls: 16,
            concavity: 0.0025,
        }
    }
}

/// Decomposes a triangle mesh into bounded convex hull point clouds for asset cooking.
pub fn decompose_mesh(
    vertices: &[Vec3],
    indices: &[[u32; 3]],
    params: DecompositionParams,
) -> Result<Vec<Vec<[f32; 3]>>, PhysicsError> {
    let points = vertices
        .iter()
        .copied()
        .map(crate::convert::to_rapier_vec)
        .collect::<Vec<_>>();
    if points.iter().any(|point| !point.is_finite()) {
        return Err(PhysicsError::NonFinite {
            field: "decomposition vertices",
        });
    }
    let settings = rapier3d::parry::transformation::vhacd::VHACDParameters {
        resolution: params.resolution,
        max_convex_hulls: params.max_hulls,
        concavity: params.concavity,
        ..rapier3d::parry::transformation::vhacd::VHACDParameters::default()
    };
    let decomposed = SharedShape::convex_decomposition_with_params(&points, indices, &settings);
    let compound = decomposed.as_compound().ok_or(PhysicsError::InvalidMesh)?;
    let mut hulls: Vec<Vec<[f32; 3]>> = Vec::with_capacity(compound.shapes().len());
    for (pose, part) in compound.shapes() {
        let hull = part
            .as_convex_polyhedron()
            .ok_or(PhysicsError::InvalidMesh)?;
        hulls.push(
            hull.points()
                .iter()
                .map(|point| {
                    let point = pose.transform_point(*point);
                    [point.x, point.y, point.z]
                })
                .collect(),
        );
    }
    if hulls.is_empty() {
        return Err(PhysicsError::InvalidMesh);
    }
    reduce_hull_count(&mut hulls, params.max_hulls)?;
    Ok(hulls)
}

fn reduce_hull_count(hulls: &mut Vec<Vec<[f32; 3]>>, max_hulls: u32) -> Result<(), PhysicsError> {
    let max_hulls = usize::try_from(max_hulls).map_err(|_| PhysicsError::InvalidMesh)?;
    while hulls.len() > max_hulls {
        let mut best: Option<(usize, usize, f32)> = None;
        for first in 0..hulls.len() {
            let Some(left) = hulls.get(first) else {
                return Err(PhysicsError::InvalidMesh);
            };
            let left_volume = ConvexShape::hull(left)?.volume();
            for second in first + 1..hulls.len() {
                let Some(right) = hulls.get(second) else {
                    return Err(PhysicsError::InvalidMesh);
                };
                let right_volume = ConvexShape::hull(right)?.volume();
                let mut merged = Vec::with_capacity(left.len() + right.len());
                merged.extend_from_slice(left);
                merged.extend_from_slice(right);
                let increase =
                    (ConvexShape::hull(&merged)?.volume() - left_volume - right_volume).max(0.0);
                if best.is_none_or(|(best_first, best_second, best_increase)| {
                    increase.total_cmp(&best_increase).is_lt()
                        || (increase == best_increase
                            && (first, second) < (best_first, best_second))
                }) {
                    best = Some((first, second, increase));
                }
            }
        }
        let Some((first, second, _)) = best else {
            return Err(PhysicsError::InvalidMesh);
        };
        let right = hulls.remove(second);
        let mut merged = hulls.remove(first);
        merged.extend(right);
        hulls.insert(first, merged);
    }
    Ok(())
}
