//! Writes versioned packs and deterministic cooking reports.
//!
//! This module owns source hashing and incremental checks so all generated files use one cache key.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use engine_assets::format::{self, Asset, AssetId, AssetIndex, AssetLocation, MeshPack};

use crate::CookError;

const HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(serde::Serialize, serde::Deserialize)]
struct CookReport {
    source_hash: u64,
    encoder: String,
    conventions: String,
    triangles_per_lod: Vec<Vec<u32>>,
    stream_bytes: Vec<Vec<[usize; 3]>>,
    texture_bytes_by_family: BTreeMap<String, u64>,
    uncooked_objects: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
/// Pipeline measurements and collision diagnostics written beside a pack.
pub(super) struct CookMetrics {
    /// Triangle count at each generated LOD.
    pub(super) triangles_per_lod: Vec<Vec<u32>>,
    /// Per-LOD position, attribute and index bytes.
    pub(super) stream_bytes: Vec<Vec<[usize; 3]>>,
    /// Estimated texture bytes for each target family.
    pub(super) texture_bytes_by_family: BTreeMap<String, u64>,
    /// Mesh objects without a collision role or explicit collision child.
    pub(super) uncooked_objects: Vec<String>,
}

/// Returns whether outputs match both the source hash and encoder revision.
pub(super) fn is_incrementally_current(
    pack_path: &Path,
    output: &Path,
    stem: &str,
    source_hash: u64,
) -> bool {
    let Ok(report_contents) = fs::read_to_string(output.join(format!("{stem}.report.ron"))) else {
        return false;
    };
    let Ok(report) = ron::from_str::<CookReport>(&report_contents) else {
        return false;
    };
    let Ok(bytes) = fs::read(pack_path) else {
        return false;
    };
    let Ok((recorded_hash, Asset::MeshPack(pack))) = format::decode(&bytes) else {
        return false;
    };
    recorded_hash == source_hash
        && pack.source_hash == source_hash
        && report.source_hash == source_hash
        && pack.textures.iter().all(|hash| {
            output
                .join("textures")
                .join(format!("{hash:016x}.ktx2"))
                .is_file()
        })
        && report.encoder == texture_encode::ENCODER_LABEL
}

/// Computes the stable FNV-1a content hash used in cooked source and texture keys.
pub(crate) fn stable_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(HASH_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(HASH_PRIME)
    })
}

fn parse_asset_id(value: &str, node: &str) -> Result<AssetId, CookError> {
    let id = value
        .strip_prefix("0x")
        .map_or_else(|| value.parse::<u64>(), |hex| u64::from_str_radix(hex, 16))
        .map_err(|_| CookError::Collision {
            object: node.to_owned(),
            reason: "asset_id must be an unsigned 64-bit integer".to_owned(),
        })?;
    AssetId::new(id).map_err(|_| CookError::Collision {
        object: node.to_owned(),
        reason: "asset_id must be non-zero".to_owned(),
    })
}

fn insert_index_entry(
    entries: &mut BTreeMap<AssetId, (String, AssetLocation)>,
    asset_id: AssetId,
    label: String,
    reference: AssetLocation,
) -> Result<(), CookError> {
    if let Some((first, _)) = entries.get(&asset_id) {
        return Err(CookError::DuplicateAssetId {
            asset_id: asset_id.get(),
            first: first.clone(),
            second: label,
        });
    }
    entries.insert(asset_id, (label, reference));
    Ok(())
}

/// Rebuilds the stable-id index from validated cooked packs in sorted file order.
pub(super) fn write_index(output: &Path, source_stems: &[&str]) -> Result<(), CookError> {
    let expected = source_stems
        .iter()
        .map(|stem| format!("{stem}.meshpack"))
        .collect::<std::collections::BTreeSet<_>>();
    let mut packs = fs::read_dir(output)?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<Result<Vec<_>, _>>()?;
    packs.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| expected.contains(name))
    });
    packs.sort();
    let mut entries = BTreeMap::<AssetId, (String, AssetLocation)>::new();
    for path in packs {
        let bytes = fs::read(&path)?;
        let (content_hash, Asset::MeshPack(pack)) = format::decode(&bytes)?;
        let pack_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(CookError::Usage)?
            .to_owned();
        let mut source_asset_id = None;
        for (node_index, node) in pack.nodes.iter().enumerate() {
            if let Some((_, value)) = node.tags.iter().find(|(key, _)| key == "source_asset_id") {
                let id = parse_asset_id(value, &node.name)?;
                if source_asset_id.is_some_and(|existing| existing != id) {
                    return Err(CookError::Collision {
                        object: node.name.clone(),
                        reason: "source_asset_id differs between nodes in one GLB".to_owned(),
                    });
                }
                source_asset_id = Some(id);
            }
            if let Some((_, value)) = node.tags.iter().find(|(key, _)| key == "asset_id") {
                let id = parse_asset_id(value, &node.name)?;
                let node_index = u32::try_from(node_index).map_err(|_| CookError::Mesh)?;
                insert_index_entry(
                    &mut entries,
                    id,
                    format!("node `{}` in `{pack_name}`", node.name),
                    AssetLocation {
                        pack: pack_name.clone(),
                        node: Some(node_index),
                        content_hash,
                    },
                )?;
            }
        }
        if let Some(id) = source_asset_id {
            insert_index_entry(
                &mut entries,
                id,
                format!("source file `{pack_name}`"),
                AssetLocation {
                    pack: pack_name.clone(),
                    node: None,
                    content_hash,
                },
            )?;
        }
    }
    let mut prefabs = Vec::new();
    collect_prefab_outputs(&output.join("prefabs"), &mut prefabs)?;
    prefabs.sort();
    let mut references = Vec::new();
    for path in prefabs {
        let bytes = fs::read(&path)?;
        let file = path.display().to_string();
        let prefab: engine_scene::PrefabDef = ron::from_str(
            std::str::from_utf8(&bytes).map_err(|error| CookError::Report(error.to_string()))?,
        )
        .map_err(|error| CookError::Report(format!("invalid prefab {file}: {error}")))?;
        let relative = path.strip_prefix(output).map_err(|_| CookError::Usage)?;
        let pack = relative
            .to_str()
            .ok_or(CookError::Usage)?
            .replace('\\', "/");
        insert_index_entry(
            &mut entries,
            prefab.asset_id,
            format!("prefab file `{pack}`"),
            AssetLocation {
                pack,
                node: None,
                content_hash: stable_hash(&bytes),
            },
        )?;
        references.push((file.clone(), prefab.model_id));
        references.extend(
            prefab
                .sounds
                .into_iter()
                .chain(prefab.effects)
                .map(|asset_id| (file.clone(), asset_id)),
        );
    }
    for (file, asset_id) in references {
        if !entries.contains_key(&asset_id) {
            return Err(CookError::MissingReference {
                object: file,
                asset_id: asset_id.get(),
            });
        }
    }
    let index: AssetIndex = entries
        .into_iter()
        .map(|(id, (_, reference))| (id, reference))
        .collect::<BTreeMap<_, _>>();
    let encoded = ron::ser::to_string_pretty(&index, ron::ser::PrettyConfig::default())
        .map_err(|error| CookError::Report(error.to_string()))?;
    fs::write(output.join("index.ron"), encoded)?;
    Ok(())
}

fn collect_prefab_outputs(
    directory: &Path,
    paths: &mut Vec<std::path::PathBuf>,
) -> Result<(), CookError> {
    if !directory.exists() {
        return Ok(());
    }
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

/// Writes the validated mesh pack and its deterministic report.
pub(super) fn write(
    output: &Path,
    stem: &str,
    source_hash: u64,
    pack: &MeshPack,
    metrics: CookMetrics,
) -> Result<(), CookError> {
    let encoded = format::encode(&Asset::MeshPack(pack.clone()), source_hash)?;
    format::decode(&encoded)?;
    fs::write(output.join(format!("{stem}.meshpack")), encoded)?;
    let report = CookReport {
        source_hash,
        encoder: texture_encode::ENCODER_LABEL.to_owned(),
        conventions: "Soft references are typed paths; caller-owned handles retain loaded assets. LOD thresholds use projected screen fraction 0.50 and 0.25; clips sample at 30 Hz with linear interpolation; absent loop metadata remains unset. Collision uses role tags or <object>_COL, with static triangle meshes and convex/decomposed prop geometry.".to_owned(),
        triangles_per_lod: metrics.triangles_per_lod,
        stream_bytes: metrics.stream_bytes,
        texture_bytes_by_family: metrics.texture_bytes_by_family,
        uncooked_objects: metrics.uncooked_objects,
    };
    fs::write(
        output.join(format!("{stem}.report.ron")),
        ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())
            .map_err(|error| CookError::Report(error.to_string()))?,
    )?;
    Ok(())
}
