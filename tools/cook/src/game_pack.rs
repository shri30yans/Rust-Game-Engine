//! Builds one deterministic game pack from cooked assets and runtime data files.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use engine_assets::{AssetPath, ContentName, ContentPack, PackError};
use engine_core::game_content_pack_path;

use crate::CookError;

const PACK_ROOTS: &[&str] = &[
    "content/cooked",
    "content/audio",
    "content/fonts",
    "data",
    "ui",
    "game/levels",
];

struct CookedPack {
    path: PathBuf,
    bytes: Vec<u8>,
    folder_sizes: BTreeMap<String, u64>,
}

/// Gathers all runtime content under a game root and writes its canonical pack.
pub fn cook_game(game_root: &Path, name: &ContentName) -> Result<PathBuf, CookError> {
    let pack = game_pack_bytes(game_root, name)?;
    if fs::read(&pack.path).ok().as_deref() != Some(pack.bytes.as_slice()) {
        fs::create_dir_all(pack.path.parent().ok_or(CookError::Usage)?)?;
        fs::write(&pack.path, pack.bytes)?;
    }
    println!(
        "cook: {} pack size: {} bytes",
        name.as_str(),
        fs::metadata(&pack.path)?.len()
    );
    for (folder, size) in pack.folder_sizes {
        println!("cook:   {folder}/: {size} bytes");
    }
    Ok(pack.path)
}

fn game_pack_bytes(game_root: &Path, name: &ContentName) -> Result<CookedPack, CookError> {
    let files = game_content_files(game_root)?;
    let mut entries = files
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(game_root).map_err(|_| CookError::Usage)?;
            let logical = relative
                .to_str()
                .ok_or(CookError::Usage)?
                .replace(std::path::MAIN_SEPARATOR, "/");
            Ok((AssetPath::new(logical)?, fs::read(path)?))
        })
        .collect::<Result<Vec<_>, CookError>>()?;
    entries.push((
        AssetPath::new("fonts/Roboto-Medium.ttf")?,
        fs::read(engine_fallback_font(game_root)?)?,
    ));
    let mut folder_sizes = BTreeMap::new();
    for (path, bytes) in &entries {
        let mut parts = path.as_str().split('/');
        let Some(root) = parts.next() else {
            return Err(CookError::Usage);
        };
        let folder = if root == "content" {
            parts
                .next()
                .map_or_else(|| root.to_owned(), |child| format!("{root}/{child}"))
        } else {
            root.to_owned()
        };
        let size = u64::try_from(bytes.len()).map_err(|_| CookError::Usage)?;
        let total = folder_sizes.entry(folder).or_insert(0_u64);
        *total = total.checked_add(size).ok_or(CookError::Usage)?;
    }
    let pack_path = game_content_pack_path(game_root, name.as_str());
    let mut bytes = Vec::new();
    ContentPack::write(entries, &mut bytes)?;
    Ok(CookedPack {
        path: pack_path,
        bytes,
        folder_sizes,
    })
}

fn game_content_files(game_root: &Path) -> Result<Vec<PathBuf>, CookError> {
    let mut files = Vec::new();
    for root in PACK_ROOTS {
        let directory = game_root.join(root);
        if directory.is_dir() {
            collect_files(game_root, &directory, &mut files)?;
        }
    }
    files.sort();
    Ok(files)
}

fn engine_fallback_font(game_root: &Path) -> Result<PathBuf, CookError> {
    let repository = game_root
        .parent()
        .and_then(Path::parent)
        .ok_or(CookError::Usage)?;
    Ok(repository.join("engine/render/fonts/Roboto-Medium.ttf"))
}

/// Hashes every file that can change a named game's cooked pack.
pub(crate) fn game_source_fingerprint(game_root: &Path) -> Result<Vec<(PathBuf, u64)>, CookError> {
    let mut files = game_content_files(game_root)?;
    files.push(engine_fallback_font(game_root)?);
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let hash = engine_assets::pack::stable_hash(&fs::read(&path)?);
            Ok((path, hash))
        })
        .collect()
}

fn collect_files(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), CookError> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(root, &path, files)?;
        } else if path.is_file()
            && !is_report(&path)
            && !path.extension().is_some_and(|ext| ext == "pak")
        {
            if path.strip_prefix(root).is_err() {
                return Err(CookError::Usage);
            }
            files.push(path);
        }
    }
    Ok(())
}

fn is_report(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".report.ron"))
}

impl From<engine_assets::InvalidContentName> for CookError {
    fn from(error: engine_assets::InvalidContentName) -> Self {
        Self::Pack(PackError::Path(error))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn game_watch_fingerprint_changes_with_runtime_content()
    -> Result<(), Box<dyn std::error::Error>> {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let repository = std::env::temp_dir().join(format!("cook-pack-watch-{unique}"));
        let game_root = repository.join("games/sample");
        let ui = game_root.join("ui/menu.ron");
        let font = repository.join("engine/render/fonts/Roboto-Medium.ttf");
        fs::create_dir_all(ui.parent().ok_or(CookError::Usage)?)?;
        fs::create_dir_all(font.parent().ok_or(CookError::Usage)?)?;
        fs::write(&ui, b"first")?;
        fs::write(&font, b"font")?;
        let first = game_source_fingerprint(&game_root)?;
        fs::write(&ui, b"second")?;
        let second = game_source_fingerprint(&game_root)?;
        fs::remove_dir_all(repository)?;
        assert_ne!(first, second);
        Ok(())
    }
}
