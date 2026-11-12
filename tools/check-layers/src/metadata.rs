//! The subset of `cargo metadata --format-version 1` that the layer check
//! reads: workspace members and their declared dependencies. Every other
//! field is ignored by serde.

use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

use crate::Error;

/// Parsed `cargo metadata` output, reduced to what the layer check needs.
#[derive(Debug, Deserialize)]
pub struct Metadata {
    /// Absolute path of the workspace root directory.
    pub workspace_root: PathBuf,
    /// Package ids of the workspace members.
    pub workspace_members: Vec<String>,
    /// Every package known to cargo, workspace members and external alike.
    pub packages: Vec<Package>,
}

/// One package entry from `cargo metadata`.
#[derive(Debug, Deserialize)]
pub struct Package {
    /// Cargo package id, matched against `workspace_members`.
    pub id: String,
    /// Package name as declared in its manifest.
    pub name: String,
    /// Absolute path of the package's `Cargo.toml`.
    pub manifest_path: PathBuf,
    /// Dependencies declared by this package.
    pub dependencies: Vec<Dependency>,
}

/// One declared dependency.
#[derive(Debug, Deserialize)]
pub struct Dependency {
    /// Real package name of the dependency, not its `rename` alias.
    pub name: String,
    /// Present for path dependencies only.
    pub path: Option<PathBuf>,
    /// `None` for normal dependencies, otherwise `"dev"` or `"build"`.
    pub kind: Option<String>,
}

/// Runs `cargo metadata --format-version 1` in the current directory and
/// parses its output. `CARGO` is honoured when set, as cargo sets it for
/// `cargo run`.
pub fn load() -> Result<Metadata, Error> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1"])
        .output()
        .map_err(Error::Spawn)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::CargoMetadata(stderr.trim().to_owned()));
    }
    serde_json::from_slice(&output.stdout).map_err(Error::ParseMetadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn parses_members_and_dependency_kinds_ignoring_extra_fields() -> TestResult {
        let text = r#"{
            "version": 1,
            "packages": [
                {
                    "id": "engine-world 0.1.0 (path+file:///ws/engine/world)",
                    "name": "engine-world",
                    "manifest_path": "/ws/engine/world/Cargo.toml",
                    "edition": "2024",
                    "dependencies": [
                        {"name": "engine-core", "path": "/ws/engine/core", "kind": null, "rename": null},
                        {"name": "hecs", "path": null, "kind": null},
                        {"name": "proptest", "path": null, "kind": "dev"}
                    ]
                }
            ],
            "workspace_members": ["engine-world 0.1.0 (path+file:///ws/engine/world)"],
            "workspace_root": "/ws"
        }"#;
        let metadata: Metadata = serde_json::from_str(text)?;
        assert_eq!(metadata.workspace_root, PathBuf::from("/ws"));
        assert_eq!(metadata.workspace_members.len(), 1);
        let package = metadata.packages.first().ok_or("no package parsed")?;
        assert_eq!(package.name, "engine-world");
        let deps = &package.dependencies;
        assert_eq!(deps.len(), 3);
        assert!(
            deps.first()
                .is_some_and(|d| d.path.is_some() && d.kind.is_none())
        );
        assert!(
            deps.get(2)
                .is_some_and(|d| d.kind.as_deref() == Some("dev"))
        );
        Ok(())
    }
}
