//! Live scene diff, staging, rollback, and readiness tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::{AppError, AssetUploader};
use engine_assets::AssetStore;
use engine_assets::format::{AssetId, AssetIndex, AssetLocation, MeshPack};
use engine_assets::{AssetPath, ContentPack, PackReader};
use engine_core::watch::POLL_INTERVAL;
use engine_physics::{PhysicsConfig, PhysicsWorld};
use engine_scene::{
    Level, PrefabRegistry, PrefabSpawn, SceneContent, SceneError, Watched, WatchedLevel,
    install_level_from_scene_content,
};
use engine_world::{Replication, World};
use glam::Vec3;
use jobs::JobPool;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::time::Instant;

use readiness::visual_pack;

/// A valid level with one shape; the edits below change its name.
fn level_text(name: &str) -> String {
    format!("(version: 1, name: \"{name}\", gravity: (0.0, -9.81, 0.0), shapes: [])")
}

fn pack_with_level(text: &str) -> ContentPack {
    let mut bytes = Vec::new();
    ContentPack::write(
        [(
            AssetPath::new("game/level.ron").expect("level path"),
            text.as_bytes().to_vec(),
        )],
        &mut bytes,
    )
    .expect("encode content pack");
    PackReader::read(Cursor::new(bytes)).expect("read content pack")
}

fn diff_level_text(instances: &str, prefab_block: &f32) -> String {
    format!(
        "(version: 1, name: \"diff\", gravity: (0.0, -9.81, 0.0), instances: {instances}, prefabs: [(id: 2, prefab_id: 3, transform: (translation: (0.0, 0.0, 0.0)), overrides: {{\"door\": {{\"open_s\": {prefab_block}}}}},)])"
    )
}

fn parse_diff_level(text: &str) -> Level {
    Level::parse(text, "diff.level.ron").expect("diff fixture")
}

fn reload_assets() -> (AssetStore, AssetUploader, JobPool) {
    (
        AssetStore::new(),
        AssetUploader::new(),
        JobPool::new(0).expect("job pool"),
    )
}

#[test]
fn stable_id_diff_reports_moved_added_removed_and_replaced_records() {
    let previous = parse_diff_level(
        "(version: 1, name: \"diff\", gravity: (0.0, -9.81, 0.0), instances: [(asset_id: 1, transform: (translation: (0.0, 0.0, 0.0))), (asset_id: 4, transform: (translation: (0.0, 0.0, 0.0)))], prefabs: [(id: 2, prefab_id: 3, transform: (translation: (0.0, 0.0, 0.0)), overrides: {\"door\": {\"open_s\": 0.5}},)])",
    );
    let next = parse_diff_level(&diff_level_text(
        "[(asset_id: 1, transform: (translation: (1.0, 0.0, 0.0))), (asset_id: 5, transform: (translation: (0.0, 0.0, 0.0)))]",
        &0.9,
    ));
    let diff = diff_levels(&previous, &next);
    assert_eq!(diff.moved.len(), 1);
    assert_eq!(diff.added.len(), 1);
    assert_eq!(diff.removed.first().map(|id| id.get()), Some(4));
    assert_eq!(diff.replaced.len(), 1);
}

#[test]
fn stable_id_diff_is_empty_for_an_unchanged_five_hundred_instance_level() {
    let instances = (0_u64..500)
        .map(|id| {
            format!(
                "(asset_id: {}, transform: (translation: (0.0, 0.0, 0.0)))",
                id + 10
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let text = diff_level_text(&format!("[{instances}]"), &0.5);
    let level = parse_diff_level(&text);
    assert_eq!(diff_levels(&level, &level), LevelDiff::default());
}

/// A world watching a logical level entry in a mounted pack.
fn watched_world(text: &str) -> (World, ContentPack) {
    let pack = pack_with_level(text);
    let watched = Watched::level(AssetPath::new("game/level.ron").expect("level path"), &pack)
        .expect("watch");
    let mut world = World::new(1).expect("world");
    world
        .insert_resource(WatchedLevel::new(watched))
        .expect("insert");
    (world, pack)
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DoorBlock {
    open_s: f32,
}

fn spawn_door(
    world: &mut World,
    placement: PrefabSpawn,
    block: DoorBlock,
) -> Result<(), SceneError> {
    world.insert(placement.entity, block)?;
    Ok(())
}

fn prefab_content() -> SceneContent {
    let index = AssetIndex::from([
        (
            AssetId::new(1).expect("model id"),
            AssetLocation {
                pack: "model.meshpack".to_owned(),
                node: Some(0),
                content_hash: 1,
            },
        ),
        (
            AssetId::new(2).expect("prefab id"),
            AssetLocation {
                pack: "door.prefab.ron".to_owned(),
                node: None,
                content_hash: 2,
            },
        ),
        (
            AssetId::new(4).expect("collider id"),
            AssetLocation {
                pack: "model.meshpack".to_owned(),
                node: Some(0),
                content_hash: 1,
            },
        ),
    ]);
    let entries = [
        (AssetPath::new("content/cooked/index.ron").expect("index path"), ron::to_string(&index).expect("encode index").into_bytes()),
        (AssetPath::new("content/cooked/model.meshpack").expect("model path"), engine_assets::format::encode(&engine_assets::format::Asset::MeshPack(visual_pack(1)), 1).expect("encode model")),
        (AssetPath::new("content/cooked/door.prefab.ron").expect("prefab path"), b"(asset_id: 2, model_id: 1, collision: Cooked, surface: \"test\", components: {\"door\": {\"open_s\": 0.25}})".to_vec()),
    ];
    let mut bytes = Vec::new();
    ContentPack::write(entries, &mut bytes).expect("encode content pack");
    let pack = PackReader::read(Cursor::new(bytes)).expect("read content pack");
    let mut prefabs = PrefabRegistry::default();
    prefabs.register("door", spawn_door).expect("register door");
    SceneContent::new(pack, prefabs, |_, _| {
        engine_scene::ColliderMetadata::default()
    })
}

#[test]
fn a_new_live_scene_shows_nothing() {
    let live = LiveScene::new();
    assert_eq!(live.message(), None);
}

#[test]
fn a_first_poll_after_startup_is_unchanged_and_shows_nothing() {
    let (mut world, pack) = watched_world(&level_text("a"));
    let mut live = LiveScene::new();
    let (mut store, uploader, pool) = reload_assets();
    store.mount_content(pack, &pool).expect("mount pack");
    live.poll_at(
        &mut world,
        &engine_scene::SceneContent::empty(),
        &mut store,
        &uploader,
        &pool,
        Instant::now() + POLL_INTERVAL,
    )
    .expect("poll");
    assert_eq!(live.message(), None);
}

#[test]
fn a_broken_edit_shows_its_error_and_a_good_edit_clears_it() {
    let (mut world, _) = watched_world(&level_text("a"));
    let mut live = LiveScene::new();
    let (mut store, uploader, pool) = reload_assets();
    store
        .mount_content(pack_with_level(&level_text("a")), &pool)
        .expect("mount initial pack");
    let t0 = Instant::now();

    store
        .mount_content(pack_with_level("(version: 1, name: "), &pool)
        .expect("mount broken edit");
    live.poll_at(
        &mut world,
        &engine_scene::SceneContent::empty(),
        &mut store,
        &uploader,
        &pool,
        t0 + POLL_INTERVAL,
    )
    .expect("poll broken");
    assert!(live.message().is_some(), "broken edit not shown");

    store
        .mount_content(pack_with_level(&level_text("b")), &pool)
        .expect("mount good edit");
    live.poll_at(
        &mut world,
        &engine_scene::SceneContent::empty(),
        &mut store,
        &uploader,
        &pool,
        t0 + POLL_INTERVAL * 2,
    )
    .expect("poll good");
    assert_eq!(live.message(), None, "good edit did not clear the banner");
}

#[test]
fn accepted_empty_metadata_edit_clears_the_banner() {
    let (mut world, _) = watched_world(&level_text("a"));
    let mut live = LiveScene::new();
    let (mut store, uploader, pool) = reload_assets();
    store
        .mount_content(pack_with_level(&level_text("a")), &pool)
        .expect("mount initial pack");
    let t0 = Instant::now();

    store
        .mount_content(pack_with_level(&level_text("b")), &pool)
        .expect("mount changed pack");
    let accepted = live
        .poll_at(
            &mut world,
            &engine_scene::SceneContent::empty(),
            &mut store,
            &uploader,
            &pool,
            t0 + POLL_INTERVAL,
        )
        .expect("poll");
    assert_eq!(
        accepted.map(|level| level.name().to_owned()),
        Some("b".to_owned())
    );
    assert_eq!(live.message(), None);
}

mod changes;
mod readiness;
