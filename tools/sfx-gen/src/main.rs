//! Command-line entry point for the deterministic placeholder sound generator.

use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args_os().nth(1).map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../games/imagine/content/audio/placeholder")
        },
        PathBuf::from,
    );
    sfx_gen::generate(&output)?;
    Ok(())
}
