//! Deterministic, game-agnostic glTF to engine asset cooker.
#![forbid(unsafe_code)]

mod args;
mod clips;
mod collision;
mod error;
mod game_pack;
mod gltf_read;
mod level;
mod mesh;
mod output;
mod texture;

#[cfg(test)]
mod tests;

pub use args::{cook_directory, run_from_args};
pub use error::CookError;
pub use game_pack::cook_game;
