//! Canonical filesystem naming for a game's checked cooked-content pack.
//!
//! The caller supplies a validated content name. This module only maps that logical name to the
//! repository's one development and cooker location; it never reads the filesystem.
use std::path::{Path, PathBuf};

/// Returns the portable filename for a validated logical content-pack name.
#[must_use]
pub fn content_pack_filename(name: &str) -> String {
    format!("{name}.pak")
}

/// Returns the canonical checked pack path beneath a game's repository root.
#[must_use]
pub fn game_content_pack_path(game_root: &Path, name: &str) -> PathBuf {
    game_root
        .join("content")
        .join("cooked")
        .join(content_pack_filename(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_pack_path_has_one_canonical_layout() {
        assert_eq!(
            game_content_pack_path(Path::new("workspace/sample"), "sample"),
            Path::new("workspace/sample/content/cooked/sample.pak")
        );
    }
}
