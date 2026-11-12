//! Bridges cooked store events to frame-boundary GPU resources.
//!
//! One upload record owns every texture and GPU pack dependency for its store handle. New packs
//! remain private until complete; reload adoption swaps retained scene references atomically.
//!
//! `uploader` is the frame step and the public handle, `queue` spends the frame's byte budget,
//! `build` holds an in-flight pack, `texture` and `mesh` upload its parts, and `state` keeps the
//! per-handle bookkeeping.

mod build;
mod client_assets;
mod mesh;
mod queue;
mod state;
mod texture;
mod types;
mod uploader;

pub use client_assets::ClientAssets;
pub use types::{GpuPack, UploadError, UploadStatus};
pub use uploader::AssetUploader;

#[cfg(test)]
mod tests;
