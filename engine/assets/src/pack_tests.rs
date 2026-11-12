//! Pack format tests cover deterministic output, integrity checks and logical path failures.
use std::io::Cursor;

use crate::{AssetPath, ContentPack, PackError, PackReader};

#[test]
fn pack_round_trips_in_stable_path_order() -> Result<(), Box<dyn std::error::Error>> {
    let entries = vec![
        (AssetPath::new("ui/home.ron")?, b"home".to_vec()),
        (AssetPath::new("meshes/pistol.meshpack")?, b"mesh".to_vec()),
    ];
    let mut bytes = Vec::new();
    ContentPack::write(entries.clone(), &mut bytes)?;
    let pack = PackReader::read(Cursor::new(&bytes))?;
    let paths = pack
        .entries()
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["meshes/pistol.meshpack", "ui/home.ron"]);
    assert_eq!(pack.load(&AssetPath::new("ui/home.ron")?)?, b"home");
    let mut second = Vec::new();
    ContentPack::write(entries, &mut second)?;
    assert_eq!(bytes, second);
    Ok(())
}

#[test]
fn pack_rejects_modified_entry_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    ContentPack::write(
        [(AssetPath::new("data/item.ron")?, b"data".to_vec())],
        &mut bytes,
    )?;
    let last = bytes.len().checked_sub(1).ok_or("empty pack")?;
    if let Some(byte) = bytes.get_mut(last) {
        *byte ^= 1;
    }
    assert!(matches!(
        PackReader::read(Cursor::new(bytes)),
        Err(PackError::Integrity(_))
    ));
    Ok(())
}

#[test]
fn missing_path_is_a_typed_error() -> Result<(), Box<dyn std::error::Error>> {
    let pack = ContentPack::default();
    let requested = AssetPath::new("missing.asset")?;
    assert!(matches!(pack.load(&requested), Err(PackError::Missing(path)) if path == requested));
    Ok(())
}

#[test]
fn paths_reject_absolute_parent_and_windows_forms() {
    for path in [
        "/absolute",
        "../escape",
        "a/../escape",
        "a\\b",
        "C:/asset",
        "a//b",
    ] {
        assert!(AssetPath::new(path).is_err(), "{path}");
    }
    assert!(AssetPath::new("meshes/pistol.meshpack").is_ok());
}
