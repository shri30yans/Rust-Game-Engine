//! Converts one cooked Blender scene into the shared validated Level format.
//!
//! Scene tags are read from cooked node records so cooker output and runtime asset references use
//! the same stable IDs and index entries.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use engine_assets::format::{AssetId, AssetIndex, MeshPack};
use engine_scene::{
    CookedColliderDef, CueDef, InstanceCollision, InstanceDef, LayoutDef, Level, LevelTransform,
    Marker, PrefabPlacement, RawLevel, VolumeDef, VolumeKind,
};
use glam::EulerRot;

use crate::CookError;

const DEFAULT_GRAVITY: [f32; 3] = [0.0, -9.81, 0.0];

/// Writes a validated `.level.ron` beside cooked packs for each source scene.
pub(super) fn write_levels(sources: &[PathBuf], output: &Path) -> Result<(), CookError> {
    let index_text = fs::read_to_string(output.join("index.ron"))?;
    let index: AssetIndex = ron::from_str(&index_text)
        .map_err(|error| CookError::Report(format!("invalid cooked index: {error}")))?;
    for source in sources {
        let stem = source
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or(CookError::Usage)?;
        let pack_path = output.join(format!("{stem}.meshpack"));
        let pack_bytes = fs::read(&pack_path)?;
        let pack = engine_assets::load::load_mesh_pack(&pack_bytes).map_err(|error| {
            CookError::Report(format!(
                "cannot read cooked pack {}: {error}",
                pack_path.display()
            ))
        })?;
        let level = build_level(stem, &pack, &index, output)?;
        let encoded = level
            .to_ron()
            .map_err(|error| CookError::Report(error.to_string()))?;
        fs::write(output.join(format!("{stem}.level.ron")), encoded)?;
    }
    Ok(())
}

fn build_level(
    level_name: &str,
    pack: &MeshPack,
    index: &AssetIndex,
    cooked_dir: &Path,
) -> Result<Level, CookError> {
    let mesh_nodes = pack
        .meshes
        .iter()
        .map(|mesh| mesh.node)
        .collect::<BTreeSet<_>>();
    let collider_nodes = pack
        .collision
        .iter()
        .map(|collision| collision.node)
        .collect::<BTreeSet<_>>();
    let mut raw = RawLevel {
        version: 1,
        name: level_name.to_owned(),
        gravity: DEFAULT_GRAVITY,
        shapes: Vec::new(),
        spawns: Vec::new(),
        waypoints: Vec::new(),
        markers: Vec::new(),
        instances: Vec::new(),
        colliders: Vec::new(),
        prefabs: Vec::new(),
        volumes: Vec::new(),
        layouts: Vec::new(),
        cues: Vec::new(),
    };
    let layout_names = scene_layout_names(pack)?;
    let mut layouts = layout_names
        .iter()
        .map(|name| LayoutDef {
            name: name.clone(),
            mover_poses: BTreeMap::new(),
            switch_states: BTreeMap::new(),
        })
        .collect::<Vec<_>>();
    for (node_index, node) in pack.nodes.iter().enumerate() {
        let node_number = u32::try_from(node_index).map_err(|_| CookError::Mesh)?;
        let tags = node.tags.iter().cloned().collect::<BTreeMap<_, _>>();
        let transform = node_transform(pack, node_index)?;
        let name = if node.name.is_empty() {
            format!("node_{node_index}")
        } else {
            node.name.clone()
        };
        let asset_id = find_id(&tags, "asset_id", &name)?;
        if let Some(id) = asset_id {
            let role = tags.get("role").map(String::as_str).unwrap_or_default();
            if mesh_nodes.contains(&node_number) && matches!(role, "static" | "mover" | "breakable")
            {
                raw.instances.push(InstanceDef {
                    asset_id: id,
                    transform,
                    material_override: None,
                    collision: collider_nodes
                        .contains(&node_number)
                        .then_some(InstanceCollision {
                            sensor: tags.get("role").is_some_and(|role| role == "trigger"),
                        }),
                });
            }
            if matches!(role, "prop" | "interact") {
                let prefab_id =
                    find_id(&tags, "prefab_id", &name)?.ok_or_else(|| CookError::Collision {
                        object: name.clone(),
                        reason: "prefab placement is missing prefab_id".to_owned(),
                    })?;
                let prefab_exists = index.get(&prefab_id).is_some_and(|location| {
                    location.node.is_none()
                        && location.pack.ends_with(".prefab.ron")
                        && cooked_dir.join(&location.pack).is_file()
                });
                if !prefab_exists {
                    return Err(CookError::MissingPrefabReference {
                        level: level_name.to_owned(),
                        entity: name.clone(),
                        prefab_id: prefab_id.get(),
                    });
                }
                raw.prefabs.push(PrefabPlacement {
                    id,
                    prefab_id,
                    transform,
                    overrides: BTreeMap::new(),
                });
            }
            if role == "mover" && mesh_nodes.contains(&node_number) {
                if layout_names.is_empty() {
                    return Err(CookError::Collision {
                        object: name.clone(),
                        reason: "mover requires at least one scene layout".to_owned(),
                    });
                }
                add_mover_layout_poses(pack, node_number, id, &name, &layout_names, &mut layouts)?;
            }
            if tags.contains_key("switch") {
                if layout_names.is_empty() {
                    return Err(CookError::Collision {
                        object: name.clone(),
                        reason: "switch requires at least one scene layout".to_owned(),
                    });
                }
                add_switch_layout_states(&tags, id, &name, &layout_names, &mut layouts)?;
            }
        }
        if let Some(id) = asset_id
            && collider_nodes.contains(&node_number)
            && !mesh_nodes.contains(&node_number)
            && !matches!(
                tags.get("role").map(String::as_str),
                Some("prop" | "interact")
            )
        {
            raw.colliders.push(CookedColliderDef {
                asset_id: id,
                transform,
                sensor: tags.get("role").is_some_and(|role| role == "trigger"),
            });
        }
        if node.name.starts_with("spawn_")
            || node.name.starts_with("player_")
            || node.name.starts_with("bot_")
        {
            raw.spawns.push(Marker {
                name,
                translation: transform.translation,
                yaw_deg: transform.rotation_deg[0],
            });
        } else if node.name.starts_with("waypoint_") {
            raw.waypoints.push(Marker {
                name,
                translation: transform.translation,
                yaw_deg: transform.rotation_deg[0],
            });
        } else if let Some(kind) = volume_kind(&node.name) {
            let id = asset_id.ok_or_else(|| CookError::Collision {
                object: name.clone(),
                reason: "volume node is missing asset_id".to_owned(),
            })?;
            raw.volumes.push(VolumeDef {
                id,
                name,
                kind,
                center: transform.translation,
                half_extents: parse_extents(tags.get("half_extents"), &node.name)?,
                tags: tags.clone(),
            });
        } else if let Some(cue_name) = node.name.strip_prefix("CUE_") {
            let layout = tags
                .get("layout")
                .cloned()
                .ok_or_else(|| CookError::Collision {
                    object: node.name.clone(),
                    reason: "cue is missing layout".to_owned(),
                })?;
            let duration_s = tags
                .get("duration_s")
                .and_then(|value| value.parse::<f32>().ok())
                .ok_or_else(|| CookError::Collision {
                    object: node.name.clone(),
                    reason: "cue duration_s must be a finite positive number".to_owned(),
                })?;
            if !duration_s.is_finite() || duration_s <= 0.0 {
                return Err(CookError::Collision {
                    object: node.name.clone(),
                    reason: "cue duration_s must be a finite positive number".to_owned(),
                });
            }
            raw.cues.push(CueDef {
                name: cue_name.to_owned(),
                layout,
                duration_s,
            });
        }
    }
    raw.layouts = layouts;
    for (object, id) in raw
        .instances
        .iter()
        .map(|instance| ("instance", instance.asset_id))
        .chain(
            raw.colliders
                .iter()
                .map(|collider| ("collider", collider.asset_id)),
        )
        .chain(
            raw.prefabs
                .iter()
                .map(|prefab| ("prefab", prefab.prefab_id)),
        )
    {
        if !index.contains_key(&id) {
            return Err(CookError::MissingReference {
                object: object.to_owned(),
                asset_id: id.get(),
            });
        }
    }
    Level::try_from(raw).map_err(|error| CookError::Level(error.to_string()))
}

fn node_transform(pack: &MeshPack, node_index: usize) -> Result<LevelTransform, CookError> {
    let matrix = pack
        .node_world_transform(node_index)
        .ok_or(CookError::Mesh)?;
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    let (yaw, pitch, roll) = rotation.to_euler(EulerRot::YXZ);
    Ok(LevelTransform {
        translation: translation.to_array(),
        rotation_deg: [yaw, pitch, roll].map(f32::to_degrees),
        scale: scale.to_array(),
    })
}

fn world_pose_node_transform(
    pack: &MeshPack,
    node_index: usize,
) -> Result<LevelTransform, CookError> {
    let node = pack.nodes.get(node_index).ok_or(CookError::Mesh)?;
    let (yaw, pitch, roll) = glam::Quat::from_array(node.rotation).to_euler(EulerRot::YXZ);
    Ok(LevelTransform {
        translation: node.translation,
        rotation_deg: [yaw, pitch, roll].map(f32::to_degrees),
        scale: node.scale,
    })
}

fn scene_layout_names(pack: &MeshPack) -> Result<Vec<String>, CookError> {
    let values = pack
        .nodes
        .iter()
        .filter_map(|node| {
            node.tags
                .iter()
                .find(|(key, _)| key == "source_layouts")
                .map(|(_, value)| value.as_str())
        })
        .collect::<BTreeSet<_>>();
    if values.len() > 1 {
        return Err(CookError::Report(
            "scene_layouts differs between exported nodes".to_owned(),
        ));
    }
    let Some(value) = values.first() else {
        return Ok(Vec::new());
    };
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let names = value.split(',').map(str::to_owned).collect::<Vec<_>>();
    if names.iter().any(|name| !valid_lower_snake_case(name))
        || names.iter().collect::<BTreeSet<_>>().len() != names.len()
    {
        return Err(CookError::Report(
            "scene layouts must be unique lower_snake_case names".to_owned(),
        ));
    }
    Ok(names)
}

fn valid_lower_snake_case(value: &str) -> bool {
    let mut previous_underscore = false;
    for (index, character) in value.chars().enumerate() {
        if character == '_' {
            if index == 0 || previous_underscore {
                return false;
            }
            previous_underscore = true;
        } else if character.is_ascii_lowercase() || (index > 0 && character.is_ascii_digit()) {
            previous_underscore = false;
        } else {
            return false;
        }
    }
    !value.is_empty() && !previous_underscore
}

fn add_mover_layout_poses(
    pack: &MeshPack,
    mover_node: u32,
    mover_id: AssetId,
    mover_name: &str,
    layout_names: &[String],
    layouts: &mut [LayoutDef],
) -> Result<(), CookError> {
    let expected = layout_names
        .iter()
        .map(|layout| format!("{mover_name}@{layout}"))
        .collect::<BTreeSet<_>>();
    let actual = pack
        .nodes
        .iter()
        .filter(|node| {
            node.parent == Some(mover_node) && node.name.starts_with(&format!("{mover_name}@"))
        })
        .map(|node| node.name.clone())
        .collect::<BTreeSet<_>>();
    if expected != actual {
        return Err(CookError::Collision {
            object: mover_name.to_owned(),
            reason: format!("layout pose children differ; expected {expected:?}, found {actual:?}"),
        });
    }
    for (layout_name, layout) in layout_names.iter().zip(layouts) {
        let pose_name = format!("{mover_name}@{layout_name}");
        let pose_index = pack
            .nodes
            .iter()
            .position(|node| node.parent == Some(mover_node) && node.name == pose_name)
            .ok_or_else(|| CookError::Collision {
                object: mover_name.to_owned(),
                reason: format!("missing layout pose `{pose_name}`"),
            })?;
        layout
            .mover_poses
            .insert(mover_id, world_pose_node_transform(pack, pose_index)?);
    }
    Ok(())
}

fn add_switch_layout_states(
    tags: &BTreeMap<String, String>,
    switch_id: AssetId,
    object_name: &str,
    layout_names: &[String],
    layouts: &mut [LayoutDef],
) -> Result<(), CookError> {
    for (layout_name, layout) in layout_names.iter().zip(layouts) {
        let field = format!("layout_{layout_name}");
        let state = tags.get(&field).ok_or_else(|| CookError::Collision {
            object: object_name.to_owned(),
            reason: format!("missing switch state `{field}`"),
        })?;
        let enabled = match state.as_str() {
            "on" => true,
            "off" => false,
            _ => {
                return Err(CookError::Collision {
                    object: object_name.to_owned(),
                    reason: format!("`{field}` must be on or off"),
                });
            }
        };
        layout.switch_states.insert(switch_id, enabled);
    }
    Ok(())
}

fn find_id(
    tags: &BTreeMap<String, String>,
    property: &str,
    object: &str,
) -> Result<Option<AssetId>, CookError> {
    tags.get(property)
        .map(|value| parse_id(value, object))
        .transpose()
}

fn parse_id(value: &str, object: &str) -> Result<AssetId, CookError> {
    let parsed = value
        .strip_prefix("0x")
        .map_or_else(|| value.parse::<u64>(), |hex| u64::from_str_radix(hex, 16))
        .map_err(|_| CookError::Collision {
            object: object.to_owned(),
            reason: "asset_id must be an unsigned 64-bit integer".to_owned(),
        })?;
    AssetId::new(parsed).map_err(|_| CookError::Collision {
        object: object.to_owned(),
        reason: "asset_id must be non-zero".to_owned(),
    })
}

fn volume_kind(name: &str) -> Option<VolumeKind> {
    if name.starts_with("zone_") {
        Some(VolumeKind::Zone)
    } else if name.starts_with("probe_") {
        Some(VolumeKind::Probe)
    } else if name.starts_with("sound_") {
        Some(VolumeKind::Sound)
    } else if name.starts_with("obj_") {
        Some(VolumeKind::Objective)
    } else if name.starts_with("trigger_") {
        Some(VolumeKind::Trigger)
    } else {
        None
    }
}

fn parse_extents(value: Option<&String>, object: &str) -> Result<[f32; 3], CookError> {
    let Some(value) = value else {
        return Ok([1.0; 3]);
    };
    let values = value
        .trim_matches(|character| matches!(character, '[' | ']' | '(' | ')'))
        .split(',')
        .map(str::trim)
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CookError::Collision {
            object: object.to_owned(),
            reason: "half_extents must contain three numbers".to_owned(),
        })?;
    values.try_into().map_err(|_| CookError::Collision {
        object: object.to_owned(),
        reason: "half_extents must contain three numbers".to_owned(),
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use engine_assets::format::{CollisionSet, Node};

    fn fixture() -> (MeshPack, AssetIndex) {
        let node = Node {
            name: "Wall".to_owned(),
            parent: None,
            translation: [3.0, 0.0, 1.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            tags: vec![
                ("asset_id".to_owned(), "42".to_owned()),
                ("role".to_owned(), "static".to_owned()),
            ],
        };
        let spawn = Node {
            name: "spawn_red".to_owned(),
            parent: None,
            translation: [0.0, 0.0, 4.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            tags: vec![("asset_id".to_owned(), "43".to_owned())],
        };
        let pack = MeshPack {
            source_hash: 1,
            meshes: vec![engine_assets::format::Mesh {
                node: 0,
                material: 0,
                lods: Vec::new(),
            }],
            materials: Vec::new(),
            nodes: vec![node, spawn],
            clips: Vec::new(),
            collision: vec![CollisionSet {
                node: 0,
                surface: "concrete".to_owned(),
                is_sensor: false,
                shapes: vec![],
            }],
            textures: Vec::new(),
        };
        let index = AssetIndex::from([
            (
                AssetId::new(42).expect("id"),
                engine_assets::format::AssetLocation {
                    pack: "arena.meshpack".to_owned(),
                    node: Some(0),
                    content_hash: 1,
                },
            ),
            (
                AssetId::new(43).expect("id"),
                engine_assets::format::AssetLocation {
                    pack: "arena.meshpack".to_owned(),
                    node: Some(1),
                    content_hash: 1,
                },
            ),
        ]);
        (pack, index)
    }

    #[test]
    fn generated_level_uses_the_shared_validated_schema() {
        let (pack, index) = fixture();
        let level = build_level("arena", &pack, &index, Path::new("target"))
            .expect("valid generated level");
        assert_eq!(level.name(), "arena");
        assert_eq!(level.instances().len(), 1);
        assert!(
            level
                .instances()
                .first()
                .is_some_and(|record| record.collision.is_some())
        );
        assert!(level.colliders().is_empty());
        assert_eq!(level.spawns().len(), 1);
        let encoded = level.to_ron().expect("serialize level");
        let round_trip = Level::parse(&encoded, "arena.level.ron").expect("read level");
        assert_eq!(round_trip, level);
    }

    #[test]
    fn generated_level_rejects_a_missing_stable_id_reference() {
        let (pack, _) = fixture();
        let error = build_level("arena", &pack, &AssetIndex::new(), Path::new("target"))
            .expect_err("missing ID");
        assert!(matches!(
            error,
            CookError::MissingReference { asset_id: 42, .. }
        ));
    }

    #[test]
    fn generated_level_rejects_a_missing_prefab_with_level_entity_and_id() {
        let (mut pack, mut index) = fixture();
        let wall = pack.nodes.first_mut().expect("fixture wall");
        wall.tags.retain(|(key, _)| key != "role");
        wall.tags.push(("role".to_owned(), "prop".to_owned()));
        wall.tags.push(("prefab_id".to_owned(), "99".to_owned()));

        index.insert(
            AssetId::new(99).expect("missing prefab ID"),
            engine_assets::format::AssetLocation {
                pack: "prefabs/turret_base.prefab.ron".to_owned(),
                node: None,
                content_hash: 0,
            },
        );
        let error = build_level("arena", &pack, &index, Path::new("target"))
            .expect_err("missing prefab file must fail");
        assert!(matches!(
            error,
            CookError::MissingPrefabReference {
                level,
                entity,
                prefab_id: 99,
            } if level == "arena" && entity == "Wall"
        ));
    }

    #[test]
    fn generated_level_cooks_layout_poses_switches_and_cues() {
        let (mut pack, mut index) = fixture();
        for node in &mut pack.nodes {
            node.tags
                .push(("source_layouts".to_owned(), "open,lockdown".to_owned()));
        }
        let mover = pack.nodes.get_mut(0).expect("mover node");
        mover.tags.retain(|(key, _)| key != "role");
        mover.tags.push(("role".to_owned(), "mover".to_owned()));
        let switch_id = AssetId::new(44).expect("switch ID");
        let switch_node = u32::try_from(pack.nodes.len()).expect("node index");
        pack.nodes.push(Node {
            name: "alarm".to_owned(),
            parent: None,
            translation: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            tags: vec![
                ("asset_id".to_owned(), "44".to_owned()),
                ("switch".to_owned(), "alarm".to_owned()),
                ("layout_open".to_owned(), "on".to_owned()),
                ("layout_lockdown".to_owned(), "off".to_owned()),
                ("source_layouts".to_owned(), "open,lockdown".to_owned()),
            ],
        });
        pack.meshes.push(engine_assets::format::Mesh {
            node: switch_node,
            material: 0,
            lods: Vec::new(),
        });
        let open_node = u32::try_from(pack.nodes.len()).expect("node index");
        pack.nodes.push(Node {
            name: "Wall@open".to_owned(),
            parent: Some(0),
            translation: [1.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            tags: Vec::new(),
        });
        pack.nodes.push(Node {
            name: "Wall@lockdown".to_owned(),
            parent: Some(0),
            translation: [2.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            tags: Vec::new(),
        });
        pack.nodes.push(Node {
            name: "CUE_lockdown".to_owned(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            tags: vec![
                ("layout".to_owned(), "lockdown".to_owned()),
                ("duration_s".to_owned(), "0.75".to_owned()),
            ],
        });
        index.insert(
            switch_id,
            engine_assets::format::AssetLocation {
                pack: "arena.meshpack".to_owned(),
                node: Some(switch_node),
                content_hash: 1,
            },
        );
        let level = build_level("arena", &pack, &index, Path::new("target"))
            .expect("generated layout level");
        assert_eq!(level.layouts().len(), 2);
        let open_layout = level.layouts().first().expect("open layout");
        let lockdown_layout = level.layouts().get(1).expect("lockdown layout");
        assert_eq!(open_layout.name, "open");
        assert_eq!(
            open_layout
                .mover_poses
                .get(&AssetId::new(42).expect("mover ID"))
                .expect("mover pose")
                .translation,
            [1.0, 0.0, 0.0]
        );
        assert!(
            *open_layout
                .switch_states
                .get(&switch_id)
                .expect("open switch state")
        );
        assert!(
            !*lockdown_layout
                .switch_states
                .get(&switch_id)
                .expect("lockdown switch state")
        );
        let cue = level.cues().first().expect("lockdown cue");
        assert_eq!(cue.name, "lockdown");
        assert_eq!(cue.duration_s, 0.75);
        assert_eq!(open_node, 3);
    }
}
