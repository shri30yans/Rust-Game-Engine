//! Pack-backed level watcher tests.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Cursor;

use engine_assets::{AssetPath, ContentPack, PackReader};
use engine_scene::{Level, Reload, Watched};

const LEVEL_PATH: &str = "game/levels/test.level.ron";

fn level_text(name: &str) -> String {
    format!("(version: 1, name: \"{name}\", gravity: (0.0, -9.81, 0.0))")
}

fn content(path: &str, text: &str) -> ContentPack {
    let entries = [(
        AssetPath::new(path).expect("valid logical path"),
        text.as_bytes().to_vec(),
    )];
    let mut bytes = Vec::new();
    ContentPack::write(entries, &mut bytes).expect("encode content pack");
    PackReader::read(Cursor::new(bytes)).expect("read content pack")
}

#[test]
fn changed_pack_entry_applies_a_valid_level() {
    let first = content(LEVEL_PATH, &level_text("first"));
    let second = content(LEVEL_PATH, &level_text("second"));
    let path = AssetPath::new(LEVEL_PATH).expect("valid level path");
    let mut watched = Watched::<Level>::level(path, &first).expect("load initial level");

    assert!(matches!(watched.poll(&first), Reload::Unchanged));
    assert!(matches!(watched.poll(&second), Reload::Applied));
    assert_eq!(watched.value().name(), "second");
    assert!(matches!(watched.poll(&second), Reload::Unchanged));
}

#[test]
fn rejected_pack_entry_keeps_the_last_good_level_and_hash() {
    let first = content(LEVEL_PATH, &level_text("first"));
    let broken = content(LEVEL_PATH, "(not a level)");
    let restored = content(LEVEL_PATH, &level_text("first"));
    let path = AssetPath::new(LEVEL_PATH).expect("valid level path");
    let mut watched = Watched::<Level>::level(path, &first).expect("load initial level");

    assert!(matches!(watched.poll(&broken), Reload::Rejected(_)));
    assert_eq!(watched.value().name(), "first");
    assert!(matches!(watched.poll(&restored), Reload::Unchanged));
}

#[test]
fn missing_level_entry_is_rejected_without_replacing_the_last_good_value() {
    let first = content(LEVEL_PATH, &level_text("first"));
    let other = content("game/levels/other.level.ron", &level_text("other"));
    let path = AssetPath::new(LEVEL_PATH).expect("valid level path");
    let mut watched = Watched::<Level>::level(path, &first).expect("load initial level");

    assert!(matches!(watched.poll(&other), Reload::Rejected(_)));
    assert_eq!(watched.value().name(), "first");
}
