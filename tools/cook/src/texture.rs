//! Decodes glTF images and creates deterministic KTX2 UASTC textures.
//!
//! Color roles are part of each content key so sRGB and linear uses cannot alias.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use gltf::{Document, image};
use rayon::ThreadPool;
use rayon::prelude::*;

use crate::CookError;
use crate::output;

/// Deduplicated image outputs and the mappings needed by materials.
pub(super) struct CookedTextures {
    /// Texture content hashes in stable order.
    pub(super) hashes: Vec<u64>,
    /// Source image and color-space role to content-hash mapping.
    pub(super) refs: BTreeMap<(usize, bool), u64>,
    /// Estimated bytes after upload for each supported GPU family.
    pub(super) bytes_by_family: BTreeMap<String, u64>,
}

struct TextureJob {
    hash: u64,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    linear: bool,
    normal_map: bool,
    image_indices: Vec<usize>,
}

/// Decodes source image storage and produces deduplicated role-specific KTX2 outputs.
pub(super) fn cook_textures(
    document: &Document,
    images: &[image::Data],
    output: &Path,
    workers: &ThreadPool,
) -> Result<CookedTextures, CookError> {
    let mut jobs = BTreeMap::<u64, TextureJob>::new();
    let mut bytes_by_family = BTreeMap::new();
    for (index, image) in images.iter().enumerate() {
        let (width, height, pixels) = image_rgba(image)?;
        let (uses_srgb, uses_linear) = image_color_roles(document, index);
        let roles = if uses_srgb && uses_linear {
            vec![false, true]
        } else {
            vec![uses_linear]
        };
        for linear in roles {
            let normal_map = linear && is_normal_image(document, index);
            let mut identity = Vec::with_capacity(pixels.len().saturating_add(1));
            identity.push(u8::from(linear));
            identity.extend_from_slice(&pixels);
            let hash = output::stable_hash(&identity);
            let job = jobs.entry(hash).or_insert_with(|| TextureJob {
                hash,
                width,
                height,
                pixels: pixels.clone(),
                linear,
                normal_map,
                image_indices: Vec::new(),
            });
            job.image_indices.push(index);
        }
    }
    for job in jobs.values() {
        accumulate_texture_sizes(&mut bytes_by_family, job.width, job.height);
    }
    let completed = workers.install(|| {
        jobs.into_values()
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|job| -> Result<TextureJob, CookError> {
                let path = output
                    .join("textures")
                    .join(format!("{:016x}.ktx2", job.hash));
                if !path.exists() {
                    let ktx2 = texture_encode::encode_ktx2(
                        &job.pixels,
                        job.width,
                        job.height,
                        job.linear,
                        job.normal_map,
                    )
                    .map_err(|_| CookError::Encode)?;
                    fs::write(&path, ktx2)?;
                }
                Ok(job)
            })
            .collect::<Result<Vec<_>, CookError>>()
    })?;
    let mut hashes = Vec::new();
    let mut refs = BTreeMap::new();
    for job in completed {
        hashes.push(job.hash);
        for image_index in job.image_indices {
            refs.insert((image_index, job.linear), job.hash);
        }
    }
    hashes.sort_unstable();
    Ok(CookedTextures {
        hashes,
        refs,
        bytes_by_family,
    })
}

fn image_rgba(image: &gltf::image::Data) -> Result<(u32, u32, Vec<u8>), CookError> {
    use gltf::image::Format;
    let pixels = match image.format {
        Format::R8G8B8A8 => image.pixels.clone(),
        Format::R8G8B8 => image
            .pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        Format::R8 => image
            .pixels
            .iter()
            .flat_map(|value| [*value, *value, *value, 255])
            .collect(),
        _ => return Err(CookError::Image),
    };
    Ok((image.width, image.height, pixels))
}
fn image_color_roles(document: &Document, image_index: usize) -> (bool, bool) {
    let mut srgb = false;
    let mut linear = false;
    for material in document.materials() {
        let pbr = material.pbr_metallic_roughness();
        srgb |= pbr
            .base_color_texture()
            .is_some_and(|texture| texture.texture().source().index() == image_index);
        srgb |= material
            .emissive_texture()
            .is_some_and(|texture| texture.texture().source().index() == image_index);
        linear |= pbr
            .metallic_roughness_texture()
            .is_some_and(|texture| texture.texture().source().index() == image_index);
        linear |= material
            .normal_texture()
            .is_some_and(|texture| texture.texture().source().index() == image_index);
    }
    (srgb, linear)
}

fn is_normal_image(document: &Document, image_index: usize) -> bool {
    document.materials().any(|material| {
        material
            .normal_texture()
            .map(|texture| texture.texture().source().index())
            == Some(image_index)
    })
}

/// Adds the complete mip-chain upload size estimates for one texture.
pub(crate) fn accumulate_texture_sizes(
    totals: &mut BTreeMap<String, u64>,
    width: u32,
    height: u32,
) {
    let mut mip_width = width.max(1);
    let mut mip_height = height.max(1);
    let mut rgba_bytes = 0_u64;
    let mut compressed_bytes = 0_u64;
    loop {
        let pixels = u64::from(mip_width) * u64::from(mip_height);
        rgba_bytes = rgba_bytes.saturating_add(pixels.saturating_mul(4));
        let blocks_x = u64::from(mip_width.div_ceil(4));
        let blocks_y = u64::from(mip_height.div_ceil(4));
        compressed_bytes =
            compressed_bytes.saturating_add(blocks_x.saturating_mul(blocks_y).saturating_mul(16));
        if mip_width == 1 && mip_height == 1 {
            break;
        }
        mip_width = (mip_width / 2).max(1);
        mip_height = (mip_height / 2).max(1);
    }
    for family in ["Bc7", "Astc4x4", "Etc2"] {
        *totals.entry(family.to_owned()).or_default() = totals
            .get(family)
            .copied()
            .unwrap_or(0)
            .saturating_add(compressed_bytes);
    }
    *totals.entry("Rgba8".to_owned()).or_default() = totals
        .get("Rgba8")
        .copied()
        .unwrap_or(0)
        .saturating_add(rgba_bytes);
}
