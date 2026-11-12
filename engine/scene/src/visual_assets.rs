//! Resolves cooked visual packs required by authored scene records before a live commit.
//!
//! The resolver follows the same index and prefab model references as scene spawning, but does
//! not create entities or renderer resources. Callers acquire the returned soft paths through
//! their normal asset store and may commit the world only after every pack is ready.

use std::path::Path;

use engine_assets::format::{AssetId, AssetIndex, AssetLocation};
use engine_assets::{AssetPath, ContentPack};

use crate::{LevelRecordDef, SceneContent, SceneError, parse_prefab};

/// One cooked mesh pack required to draw a candidate live-scene record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookedVisualAsset {
    pack_path: AssetPath,
    source_hash: u64,
}

impl CookedVisualAsset {
    /// The cooked mesh-pack soft path to acquire through the asset store.
    #[must_use]
    pub fn pack_path(&self) -> &AssetPath {
        &self.pack_path
    }

    /// The exact cooked revision required before the candidate can be committed.
    #[must_use]
    pub const fn source_hash(&self) -> u64 {
        self.source_hash
    }
}

/// Resolves the deduplicated cooked visual packs required by `records`.
///
/// Primitive shapes and collision-only records require no visual pack. Prefab placements resolve
/// their prefab data record first, then the prefab's model record. Every returned path is checked
/// to remain below the scene content's cooked directory.
///
/// # Errors
///
/// Returns a scene error when the cooked index, prefab, model reference, or registered prefab
/// block is invalid.
pub fn resolve_visual_assets(
    records: &[LevelRecordDef],
    pack: &ContentPack,
    content: &SceneContent,
) -> Result<Vec<CookedVisualAsset>, SceneError> {
    if !records.iter().any(|record| {
        matches!(
            record,
            LevelRecordDef::Instance(_) | LevelRecordDef::Prefab(_)
        )
    }) {
        return Ok(Vec::new());
    }
    let index = load_index(pack)?;
    let mut assets = Vec::new();
    for record in records {
        let model_id = match record {
            LevelRecordDef::Instance(instance) => instance.asset_id,
            LevelRecordDef::Prefab(placement) => {
                let prefab_location = location(&index, placement.prefab_id)?;
                if prefab_location.node.is_some() {
                    return Err(reference_error(
                        placement.prefab_id,
                        "prefab ID must reference a prefab data file",
                    ));
                }
                let prefab_path = pack_path(placement.prefab_id, prefab_location)?;
                let prefab_text =
                    pack.load_text(&prefab_path)
                        .map_err(|error| SceneError::Prefab {
                            file: prefab_path.to_string(),
                            field: "file".to_owned(),
                            reason: error.to_string(),
                        })?;
                let prefab = parse_prefab(prefab_text, prefab_path.as_str())?;
                if prefab.asset_id != placement.prefab_id {
                    return Err(SceneError::Prefab {
                        file: prefab_path.to_string(),
                        field: "asset_id".to_owned(),
                        reason: "does not match the placement prefab_id".to_owned(),
                    });
                }
                content
                    .prefabs()
                    .validate(prefab_path.as_str(), &prefab, &placement.overrides)?;
                prefab.model_id
            }
            LevelRecordDef::Shape(_) | LevelRecordDef::Collider(_) => continue,
        };
        let model = location(&index, model_id)?;
        if model.node.is_none() {
            return Err(reference_error(
                model_id,
                "model ID names a source file, not an object node",
            ));
        }
        let pack_path = pack_path(model_id, model)?;
        if !assets
            .iter()
            .any(|asset: &CookedVisualAsset| asset.pack_path == pack_path)
        {
            assets.push(CookedVisualAsset {
                pack_path,
                source_hash: model.content_hash,
            });
        }
    }
    Ok(assets)
}

fn load_index(content: &ContentPack) -> Result<AssetIndex, SceneError> {
    let path =
        AssetPath::new("content/cooked/index.ron").map_err(|error| SceneError::AssetIndex {
            path: "content/cooked/index.ron".into(),
            reason: error.to_string(),
        })?;
    let text = content
        .load_text(&path)
        .map_err(|error| SceneError::AssetIndex {
            path: path.as_str().into(),
            reason: error.to_string(),
        })?;
    ron::from_str(text).map_err(|error| SceneError::AssetIndex {
        path: path.as_str().into(),
        reason: error.to_string(),
    })
}

fn location(index: &AssetIndex, asset_id: AssetId) -> Result<&AssetLocation, SceneError> {
    index
        .get(&asset_id)
        .ok_or_else(|| reference_error(asset_id, "asset ID is not present in cooked/index.ron"))
}

fn pack_path(asset_id: AssetId, location: &AssetLocation) -> Result<AssetPath, SceneError> {
    let relative = Path::new(&location.pack);
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(reference_error(
            asset_id,
            "index pack path must stay inside the cooked directory",
        ));
    }
    AssetPath::new(format!("content/cooked/{}", relative.display()))
        .map_err(|error| reference_error(asset_id, &error.to_string()))
}

fn reference_error(asset_id: AssetId, reason: &str) -> SceneError {
    SceneError::CookedAssetReference {
        asset_id,
        reason: reason.to_owned(),
    }
}
