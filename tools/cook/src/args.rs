//! Handles cooker arguments and coordinates independent pipeline stages.
//!
//! Input paths are discovered and processed in sorted order for reproducible output.
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use engine_assets::{ContentName, format::MeshPack};
use rayon::ThreadPool;

use crate::{CookError, clips, gltf_read, mesh, output, texture};

/// Parses source and output arguments and cooks every GLB in sorted path order.
pub fn run_from_args(args: impl IntoIterator<Item = OsString>) -> Result<(), CookError> {
    let mut values = args.into_iter().collect::<Vec<_>>().into_iter();
    let first = values.next().ok_or(CookError::Usage)?;
    if first == "--watch" {
        let watched = values.next().ok_or(CookError::Usage)?;
        if values.next().is_some() {
            return Err(CookError::Usage);
        }
        let name = ContentName::new(watched.to_string_lossy().into_owned())
            .map_err(|_| CookError::Usage)?;
        let root = named_game_root(&name)?;
        if !root.is_dir() {
            return Err(CookError::Usage);
        }
        return watch_game(&root, &name);
    }
    let Some(output) = values.next() else {
        let name = first.to_string_lossy().into_owned();
        let name = ContentName::new(name).map_err(|_| CookError::Usage)?;
        let root = named_game_root(&name)?;
        if !root.is_dir() {
            return Err(CookError::Usage);
        }
        cook_named_game(&root, &name)?;
        return Ok(());
    };
    let source = PathBuf::from(first);
    let output = PathBuf::from(output);
    if values.next().is_some() {
        return Err(CookError::Usage);
    }
    cook_directory(&source, &output)
}

fn named_game_root(name: &ContentName) -> Result<PathBuf, CookError> {
    Ok(std::env::current_dir()?.join("games").join(name.as_str()))
}

fn cook_named_game(root: &Path, name: &ContentName) -> Result<(), CookError> {
    let cooked = root.join("content/cooked");
    if root.join("content/source").is_dir() {
        cook_directory(&root.join("content/source"), &cooked)?;
    } else if root.join("content/weapons_batch1/export").is_dir() {
        cook_directory(
            &root.join("content/weapons_batch1/export"),
            &cooked.join("weapons"),
        )?;
    }
    crate::game_pack::cook_game(root, name)?;
    Ok(())
}

const WATCH_INTERVAL: Duration = Duration::from_millis(250);

fn watch_game(root: &Path, name: &ContentName) -> Result<(), CookError> {
    cook_named_game(root, name)?;
    let mut previous = game_watch_fingerprint(root)?;
    loop {
        thread::sleep(WATCH_INTERVAL);
        let current = game_watch_fingerprint(root)?;
        if current != previous {
            cook_named_game(root, name)?;
            previous = game_watch_fingerprint(root)?;
        }
    }
}

fn game_watch_fingerprint(root: &Path) -> Result<Vec<(PathBuf, u64)>, CookError> {
    let mut fingerprint = crate::game_pack::game_source_fingerprint(root)?;
    let authored = root.join("content/source");
    let exported = root.join("content/weapons_batch1/export");
    if authored.is_dir() {
        fingerprint.extend(source_fingerprint(&authored)?);
    } else if exported.is_dir() {
        fingerprint.extend(source_fingerprint(&exported)?);
    }
    fingerprint.sort_by(|left, right| left.0.cmp(&right.0));
    fingerprint.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    Ok(fingerprint)
}

fn source_fingerprint(source: &Path) -> Result<Vec<(PathBuf, u64)>, CookError> {
    let mut paths = Vec::new();
    collect_sources(source, &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path)?;
            Ok((path, output::stable_hash(&bytes)))
        })
        .collect()
}

fn collect_sources(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), CookError> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "cooked") {
                continue;
            }
            collect_sources(&path, paths)?;
        } else if path.extension().is_some_and(|extension| extension == "glb")
            || path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".prefab.ron"))
        {
            paths.push(path);
        }
    }
    Ok(())
}

/// Cooks each GLB in a source directory into deterministic mesh packs and textures.
pub fn cook_directory(source: &Path, output: &Path) -> Result<(), CookError> {
    let workers = rayon::ThreadPoolBuilder::new()
        .build()
        .map_err(|_| CookError::WorkerPool)?;
    fs::create_dir_all(output.join("textures"))?;
    let mut paths = fs::read_dir(source)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "glb"))
        .collect::<Vec<_>>();
    paths.sort();
    for path in &paths {
        cook_file(path, output, &workers)?;
    }
    cook_prefabs(source, output)?;
    let stems = paths
        .iter()
        .filter_map(|path| path.file_stem())
        .filter_map(|stem| stem.to_str())
        .collect::<Vec<_>>();
    remove_stale_outputs(output, &stems)?;
    output::write_index(output, &stems)?;
    crate::level::write_levels(&paths, output)?;
    Ok(())
}

fn remove_stale_outputs(output: &Path, stems: &[&str]) -> Result<(), CookError> {
    let expected = stems
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    for entry in fs::read_dir(output)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let stem = name
            .strip_suffix(".report.ron")
            .or_else(|| name.strip_suffix(".level.ron"))
            .or_else(|| name.strip_suffix(".meshpack"));
        if stem.is_some_and(|stem| !expected.contains(stem)) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn cook_prefabs(source: &Path, output: &Path) -> Result<(), CookError> {
    let mut paths = Vec::new();
    collect_prefabs(source, &mut paths)?;
    paths.sort();
    let prefab_root = output.join("prefabs");
    fs::create_dir_all(&prefab_root)?;
    let mut expected = Vec::with_capacity(paths.len());
    for path in paths {
        let text = fs::read_to_string(&path)?;
        engine_scene::parse_prefab(&text, &path.display().to_string())
            .map_err(|error| CookError::Report(error.to_string()))?;
        let prefab_source = source.join("prefabs");
        let relative = path
            .strip_prefix(&prefab_source)
            .or_else(|_| path.strip_prefix(source))
            .map_err(|_| CookError::Usage)?;
        let output_path = prefab_root.join(relative);
        expected.push(output_path.clone());
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = fs::read(&path)?;
        if fs::read(&output_path).ok().as_deref() != Some(bytes.as_slice()) {
            fs::write(output_path, bytes)?;
        }
    }
    let mut existing = Vec::new();
    collect_prefab_outputs(&prefab_root, &mut existing)?;
    for path in existing {
        if !expected.contains(&path) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn collect_prefab_outputs(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), CookError> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_prefab_outputs(&path, paths)?;
        } else if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".prefab.ron"))
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn collect_prefabs(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), CookError> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "cooked") {
                continue;
            }
            collect_prefabs(&path, paths)?;
        } else if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".prefab.ron"))
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn cook_file(path: &Path, output: &Path, workers: &ThreadPool) -> Result<(), CookError> {
    let source_bytes = fs::read(path)?;
    let source_hash = output::stable_hash(&source_bytes);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or(CookError::Usage)?;
    let pack_path = output.join(format!("{stem}.meshpack"));
    if output::is_incrementally_current(&pack_path, output, stem, source_hash) {
        return Ok(());
    }

    let (document, buffers, images) = gltf::import(path)?;
    let cooked_textures = texture::cook_textures(&document, &images, output, workers)?;
    let (nodes, node_map) = gltf_read::read_nodes(&document)?;
    let materials = gltf_read::read_materials(&document, &cooked_textures.refs);
    let mesh_build = mesh::read_meshes(&document, &buffers, &node_map)?;
    let collision = crate::collision::cook(&document, &buffers, &node_map, &nodes)?;
    let clips = clips::cook_animations(&document, &buffers, &node_map)?;
    let pack = MeshPack {
        source_hash,
        meshes: mesh_build.meshes,
        materials,
        nodes,
        clips,
        collision: collision.sets,
        textures: cooked_textures.hashes,
    };
    output::write(
        output,
        stem,
        source_hash,
        &pack,
        output::CookMetrics {
            triangles_per_lod: mesh_build.triangles_per_lod,
            stream_bytes: mesh_build.stream_bytes,
            texture_bytes_by_family: cooked_textures.bytes_by_family,
            uncooked_objects: collision.uncooked_objects,
        },
    )
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn source_fingerprint_uses_contents_and_ignores_generated_cooked_files() {
        let root = std::env::temp_dir().join(format!("cook-watch-{}", std::process::id()));
        let prefab_dir = root.join("prefabs");
        let cooked_dir = root.join("cooked");
        fs::create_dir_all(&prefab_dir).expect("prefab dir");
        fs::create_dir_all(&cooked_dir).expect("cooked dir");
        let prefab = prefab_dir.join("crate.prefab.ron");
        fs::write(&prefab, "first").expect("write source");
        fs::write(cooked_dir.join("generated.prefab.ron"), "generated").expect("write output");
        let initial = source_fingerprint(&root).expect("fingerprint");
        fs::write(&prefab, "second").expect("update source");
        let updated = source_fingerprint(&root).expect("updated fingerprint");
        assert_ne!(initial, updated);
        assert_eq!(updated.len(), 1);
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
