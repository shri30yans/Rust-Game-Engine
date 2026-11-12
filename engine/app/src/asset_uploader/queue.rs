//! The FIFO upload queue: uploads a pack's next parts in order while they fit the frame's byte
//! budget. Part order is textures first, then meshes.

use render::Gfx;

use crate::error::AppError;

use super::build::PackBuild;
use super::mesh::upload_mesh;
use super::texture::upload_texture;

/// Uploads the build's next parts while each fits in what is left of `byte_budget`. `uploaded`
/// carries the bytes already spent this frame across packs.
pub(super) fn upload_within_budget(
    build: &mut PackBuild,
    gfx: &mut Gfx,
    uploaded: &mut u64,
    byte_budget: u64,
) -> Result<(), AppError> {
    while let Some(next_bytes) = build.upload_bytes(build.next_part) {
        if uploaded.saturating_add(next_bytes) > byte_budget {
            break;
        }
        let Some(next_uploaded) = upload_part(build, gfx, build.next_part)? else {
            break;
        };
        *uploaded = uploaded.saturating_add(next_uploaded);
        build.next_part = build.next_part.saturating_add(1);
    }
    Ok(())
}

/// Uploads one part: a texture below the texture count, otherwise a mesh.
fn upload_part(build: &mut PackBuild, gfx: &mut Gfx, part: usize) -> Result<Option<u64>, AppError> {
    if part < build.textures.len() {
        upload_texture(build, gfx, part)
    } else {
        upload_mesh(build, gfx, part)
    }
}
