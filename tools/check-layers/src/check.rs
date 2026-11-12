//! Layer and zone judgement. [`find_violations`] is pure over parsed metadata
//! and a layer table, so every rule is tested with fixtures and no I/O.
//!
//! Enforces numeric layers and folder zones: layer edges, zone edges, per-game isolation, and (with scan)
//! forbidden text.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::metadata::{Metadata, Package};
use crate::rules::{LayerConfig, ZoneName, ZoneScope, game_prefix};

/// Invalid manifest paths when resolving workspace-relative crate directories.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
    /// A package manifest has no parent directory.
    #[error("package manifest `{}` has no parent directory", manifest.display())]
    ManifestWithoutParent {
        /// Manifest path from metadata.
        manifest: PathBuf,
    },
    /// A package directory is not under the workspace root.
    #[error(
        "package path `{}` is outside workspace root `{}`",
        path.display(),
        root.display()
    )]
    OutsideWorkspace {
        /// Package directory on disk.
        path: PathBuf,
        /// Workspace root from metadata.
        root: PathBuf,
    },
    /// A scanned file is not under the crate root being walked.
    #[error(
        "source file `{}` is not under crate root `{}`",
        path.display(),
        crate_root.display()
    )]
    NotUnderCrateRoot {
        /// File path found while walking.
        path: PathBuf,
        /// Crate root directory.
        crate_root: PathBuf,
    },
}

/// One problem found in the workspace graph or source tree.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Violation {
    /// A workspace crate matches no rule, so it has no layer.
    #[error("crate `{package}` (at `{dir}`) matches no rule in layers.json; assign it a layer")]
    Unmatched {
        /// Package name.
        package: String,
        /// Workspace-relative directory.
        dir: String,
    },
    /// A workspace crate matches more than one rule, so its layer is ambiguous.
    #[error(
        "crate `{package}` (at `{dir}`) matches several rules ({patterns}); make the rules disjoint"
    )]
    Ambiguous {
        /// Package name.
        package: String,
        /// Workspace-relative directory.
        dir: String,
        /// Patterns of every matching rule, comma-separated.
        patterns: String,
    },
    /// A workspace crate matches no zone pattern.
    #[error(
        "crate `{package}` (at `{dir}`) matches no zone in layers.json; add a zone pattern for it"
    )]
    NoZone {
        /// Package name.
        package: String,
        /// Workspace-relative directory.
        dir: String,
    },
    /// A workspace crate matches more than one zone pattern.
    #[error(
        "crate `{package}` (at `{dir}`) matches several zones ({zones}); make zone patterns disjoint"
    )]
    AmbiguousZone {
        /// Package name.
        package: String,
        /// Workspace-relative directory.
        dir: String,
        /// Zone names that matched.
        zones: String,
    },
    /// A crate depends on a crate in a higher layer.
    #[error(
        "`{from}` (layer {from_layer}) has a {kind} dependency on `{to}` (layer {to_layer}); dependencies may only point to the same or a lower layer"
    )]
    UpwardEdge {
        /// Depending package.
        from: String,
        /// Layer of the depending package.
        from_layer: u32,
        /// Package being depended on.
        to: String,
        /// Layer of the package being depended on.
        to_layer: u32,
        /// `normal`, `dev` or `build`.
        kind: &'static str,
    },
    /// A crate depends on another crate in the same layer without an allowed pair.
    #[error(
        "`{from}` has a {kind} dependency on `{to}`, both in layer {layer}; same-layer dependencies need an allowed pair in layers.json"
    )]
    SameLayerEdge {
        /// Depending package.
        from: String,
        /// Package being depended on.
        to: String,
        /// Shared layer.
        layer: u32,
        /// `normal`, `dev` or `build`.
        kind: &'static str,
    },
    /// A dependency crosses zone policy.
    #[error(
        "`{from}` (zone {from_zone}) has a {kind} dependency on `{to}` (zone {to_zone}); add `{to_zone}` to the `{from_zone}` zone's may_use list or remove the dependency"
    )]
    ZoneEdge {
        /// Depending package.
        from: String,
        /// Zone of the depending package.
        from_zone: String,
        /// Package being depended on.
        to: String,
        /// Zone of the dependency.
        to_zone: String,
        /// `normal`, `dev` or `build`.
        kind: &'static str,
    },
    /// A game crate depends on another game's crate.
    #[error(
        "`{from}` (game `{from_game}`) has a {kind} dependency on `{to}` (game `{to_game}`); crates under games/ may depend only on the same game"
    )]
    CrossGameEdge {
        /// Depending package.
        from: String,
        /// Game folder name of the depending crate.
        from_game: String,
        /// Package being depended on.
        to: String,
        /// Game folder name of the dependency.
        to_game: String,
        /// `normal`, `dev` or `build`.
        kind: &'static str,
    },
    /// A crate has a path dependency outside the workspace, which has no layer.
    #[error(
        "`{from}` has a {kind} path dependency on `{to}`, which is outside the workspace and cannot be layer-checked"
    )]
    ExternalPath {
        /// Depending package.
        from: String,
        /// Path dependency outside the workspace.
        to: String,
        /// `normal`, `dev` or `build`.
        kind: &'static str,
    },
    /// Source text contains a forbidden substring for its zone.
    #[error(
        "file `{file}` line {line} in zone `{zone}` contains forbidden text `{text}`; remove paths into other folders"
    )]
    ForbiddenText {
        /// Workspace-relative file path.
        file: String,
        /// 1-based line number.
        line: usize,
        /// Forbidden substring that matched.
        text: String,
        /// Zone that forbids the text.
        zone: String,
    },
}

/// Crate directory and zone for source scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateZone {
    /// Workspace-relative directory.
    pub dir: String,
    /// Assigned zone.
    pub zone: ZoneName,
    /// Forbidden substrings for that zone.
    pub forbid_text: Vec<String>,
}

/// Collects workspace members that belong to exactly one zone (for text scan).
pub fn crate_zones_for_scan(
    metadata: &Metadata,
    config: &LayerConfig,
) -> Result<Vec<CrateZone>, CheckError> {
    if config.zones.is_empty() {
        return Ok(Vec::new());
    }
    let members: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|pkg| metadata.workspace_members.contains(&pkg.id))
        .collect();
    let mut out = Vec::new();
    for pkg in members {
        let dir = relative_dir(&metadata.workspace_root, &pkg.manifest_path)?;
        let zones = config.matching_zones(&dir);
        let [zone] = zones.as_slice() else {
            continue;
        };
        out.push(CrateZone {
            dir,
            zone: zone.name.clone(),
            forbid_text: zone.forbid_text.clone(),
        });
    }
    Ok(out)
}

/// Checks every workspace crate and path dependency against `config`.
///
/// Returns all violations, sorted by package name in metadata order within
/// each check. An empty vector means the graph is clean.
pub fn find_violations(
    metadata: &Metadata,
    config: &LayerConfig,
) -> Result<Vec<Violation>, CheckError> {
    let mut members: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|pkg| metadata.workspace_members.contains(&pkg.id))
        .collect();
    members.sort_by(|a, b| a.name.cmp(&b.name));
    let member_names: BTreeSet<&str> = members.iter().map(|pkg| pkg.name.as_str()).collect();

    let mut violations = Vec::new();
    let mut layer_of: BTreeMap<&str, u32> = BTreeMap::new();
    let mut zone_of: BTreeMap<&str, ZoneName> = BTreeMap::new();
    let mut dir_of: BTreeMap<&str, String> = BTreeMap::new();
    let check_zones = !config.zones.is_empty();

    for pkg in &members {
        let dir = relative_dir(&metadata.workspace_root, &pkg.manifest_path)?;
        dir_of.insert(pkg.name.as_str(), dir.clone());
        let rules = config.matching_rules(&dir);
        match rules.as_slice() {
            [] => violations.push(Violation::Unmatched {
                package: pkg.name.clone(),
                dir: dir.clone(),
            }),
            [rule] => {
                layer_of.insert(pkg.name.as_str(), rule.layer);
            }
            many => violations.push(Violation::Ambiguous {
                package: pkg.name.clone(),
                dir: dir.clone(),
                patterns: many
                    .iter()
                    .map(|rule| rule.pattern.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }

        if check_zones {
            let zones = config.matching_zones(&dir);
            match zones.as_slice() {
                [] => violations.push(Violation::NoZone {
                    package: pkg.name.clone(),
                    dir: dir.clone(),
                }),
                [zone] => {
                    zone_of.insert(pkg.name.as_str(), zone.name.clone());
                }
                many => violations.push(Violation::AmbiguousZone {
                    package: pkg.name.clone(),
                    dir,
                    zones: many
                        .iter()
                        .map(|zone| zone.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                }),
            }
        }
    }

    for pkg in &members {
        let from_layer = layer_of.get(pkg.name.as_str()).copied();
        let from_zone = zone_of.get(pkg.name.as_str());
        let from_dir = dir_of.get(pkg.name.as_str()).map(String::as_str);
        for dep in pkg.dependencies.iter().filter(|dep| dep.path.is_some()) {
            let kind = dep_kind(dep.kind.as_deref());
            if !member_names.contains(dep.name.as_str()) {
                violations.push(Violation::ExternalPath {
                    from: pkg.name.clone(),
                    to: dep.name.clone(),
                    kind,
                });
                continue;
            }
            let to_layer = layer_of.get(dep.name.as_str()).copied();
            if let (Some(from_layer), Some(to_layer)) = (from_layer, to_layer) {
                if to_layer > from_layer {
                    violations.push(Violation::UpwardEdge {
                        from: pkg.name.clone(),
                        from_layer,
                        to: dep.name.clone(),
                        to_layer,
                        kind,
                    });
                } else if to_layer == from_layer && !config.allows_same_layer(&pkg.name, &dep.name)
                {
                    violations.push(Violation::SameLayerEdge {
                        from: pkg.name.clone(),
                        to: dep.name.clone(),
                        layer: from_layer,
                        kind,
                    });
                }
            }

            if let Some(from_zone) = from_zone {
                let Some(to_zone) = zone_of.get(dep.name.as_str()) else {
                    continue;
                };
                if !config.zone_allows_edge(from_zone, to_zone) {
                    violations.push(Violation::ZoneEdge {
                        from: pkg.name.clone(),
                        from_zone: from_zone.as_str().to_owned(),
                        to: dep.name.clone(),
                        to_zone: to_zone.as_str().to_owned(),
                        kind,
                    });
                } else if let (Some(from_dir), Some(to_dir)) =
                    (from_dir, dir_of.get(dep.name.as_str()).map(String::as_str))
                    && per_game_cross(config, from_zone, to_zone, from_dir, to_dir)
                {
                    let (Some(from_game), Some(to_game)) =
                        (game_prefix(from_dir), game_prefix(to_dir))
                    else {
                        continue;
                    };
                    violations.push(Violation::CrossGameEdge {
                        from: pkg.name.clone(),
                        from_game: from_game.to_owned(),
                        to: dep.name.clone(),
                        to_game: to_game.to_owned(),
                        kind,
                    });
                }
            }
        }
    }
    Ok(violations)
}

fn per_game_cross(
    config: &LayerConfig,
    from_zone: &ZoneName,
    to_zone: &ZoneName,
    from_dir: &str,
    to_dir: &str,
) -> bool {
    let from_per = config
        .zone_by_name(from_zone)
        .is_some_and(|zone| zone.scope == ZoneScope::PerGame);
    let to_per = config
        .zone_by_name(to_zone)
        .is_some_and(|zone| zone.scope == ZoneScope::PerGame);
    if !(from_per && to_per) {
        return false;
    }
    match (game_prefix(from_dir), game_prefix(to_dir)) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    }
}

/// Workspace-relative directory of a package, with `/` separators.
pub(crate) fn relative_dir(root: &Path, manifest: &Path) -> Result<String, CheckError> {
    let Some(dir) = manifest.parent() else {
        return Err(CheckError::ManifestWithoutParent {
            manifest: manifest.to_path_buf(),
        });
    };
    let relative = dir
        .strip_prefix(root)
        .map_err(|_| CheckError::OutsideWorkspace {
            path: dir.to_path_buf(),
            root: root.to_path_buf(),
        })?;
    Ok(relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

/// Maps cargo's dependency `kind` to the label used in messages.
fn dep_kind(kind: Option<&str>) -> &'static str {
    match kind {
        Some("dev") => "dev",
        Some("build") => "build",
        _ => "normal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::{Value, json};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const ROOT: &str = "/ws";

    fn zones_json() -> Value {
        json!([
            { "name": "engine", "patterns": ["engine/*"], "may_use": ["engine"], "scope": "shared", "forbid_text": ["games/"] },
            { "name": "kits", "patterns": ["kits/*"], "may_use": ["engine", "kits"], "scope": "shared", "forbid_text": ["games/"] },
            { "name": "tools", "patterns": ["tools/*"], "may_use": ["engine", "tools"], "scope": "shared" },
            { "name": "games", "patterns": ["games/*/game", "games/*/modes/*"], "may_use": ["engine", "kits", "games"], "scope": "per_game" }
        ])
    }

    /// Layer table with the rules used by every fixture below.
    fn config(same_layer_allowed: Value) -> Result<LayerConfig, serde_json::Error> {
        serde_json::from_value(json!({
            "rules": [
                {"pattern": "engine/core", "layer": 0},
                {"pattern": "engine/world", "layer": 1},
                {"pattern": "engine/platform", "layer": 1},
                {"pattern": "engine/render", "layer": 2},
                {"pattern": "kits/*", "layer": 3},
                {"pattern": "games/*/game", "layer": 5},
                {"pattern": "games/*/modes/*", "layer": 5},
                {"pattern": "tools/*", "layer": 5}
            ],
            "zones": zones_json(),
            "same_layer_allowed": same_layer_allowed
        }))
    }

    /// One package in `dir`. Each dependency is `(name, is_path, kind)`.
    fn package(name: &str, dir: &str, deps: &[(&str, bool, Option<&str>)]) -> Value {
        let deps: Vec<Value> = deps
            .iter()
            .map(|&(dep, is_path, kind)| {
                json!({
                    "name": dep,
                    "path": if is_path { json!(format!("{ROOT}/{dep}")) } else { Value::Null },
                    "kind": kind,
                })
            })
            .collect();
        json!({
            "id": name,
            "name": name,
            "manifest_path": format!("{ROOT}/{dir}/Cargo.toml"),
            "dependencies": deps,
        })
    }

    /// Metadata whose workspace members are the packages named in `members`.
    fn metadata(packages: Vec<Value>, members: &[&str]) -> Result<Metadata, serde_json::Error> {
        serde_json::from_value(json!({
            "workspace_root": ROOT,
            "workspace_members": members,
            "packages": packages,
        }))
    }

    #[test]
    fn valid_graph_has_no_violations() -> TestResult {
        let packages = vec![
            package("engine-core", "engine/core", &[]),
            package(
                "engine-world",
                "engine/world",
                &[("engine-core", true, None)],
            ),
            package(
                "engine-platform",
                "engine/platform",
                &[("engine-core", true, None)],
            ),
            package(
                "engine-render",
                "engine/render",
                &[
                    ("engine-core", true, None),
                    ("engine-platform", true, None),
                    ("serde", false, None),
                ],
            ),
            package("kit-fps", "kits/fps", &[("engine-render", true, None)]),
            package(
                "game",
                "games/chaos/game",
                &[
                    ("kit-fps", true, None),
                    ("engine-render", true, Some("dev")),
                ],
            ),
        ];
        let members = [
            "engine-core",
            "engine-world",
            "engine-platform",
            "engine-render",
            "kit-fps",
            "game",
        ];
        let violations = find_violations(&metadata(packages, &members)?, &config(json!([]))?)?;
        assert!(violations.is_empty(), "unexpected: {violations:?}");
        Ok(())
    }

    #[test]
    fn upward_edge_is_rejected_for_normal_and_dev_dependencies() -> TestResult {
        let packages = vec![
            package(
                "engine-core",
                "engine/core",
                &[("engine-render", true, None)],
            ),
            package("engine-render", "engine/render", &[]),
            package(
                "engine-platform",
                "engine/platform",
                &[("kit-fps", true, Some("dev"))],
            ),
            package("kit-fps", "kits/fps", &[]),
        ];
        let members = ["engine-core", "engine-render", "engine-platform", "kit-fps"];
        let violations = find_violations(&metadata(packages, &members)?, &config(json!([]))?)?;
        assert_eq!(
            violations,
            vec![
                Violation::UpwardEdge {
                    from: "engine-core".into(),
                    from_layer: 0,
                    to: "engine-render".into(),
                    to_layer: 2,
                    kind: "normal",
                },
                Violation::UpwardEdge {
                    from: "engine-platform".into(),
                    from_layer: 1,
                    to: "kit-fps".into(),
                    to_layer: 3,
                    kind: "dev",
                },
                Violation::ZoneEdge {
                    from: "engine-platform".into(),
                    from_zone: "engine".into(),
                    to: "kit-fps".into(),
                    to_zone: "kits".into(),
                    kind: "dev",
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn engine_depending_on_kit_is_zone_edge() -> TestResult {
        let packages = vec![
            package("engine-core", "engine/core", &[("kit-fps", true, None)]),
            package("kit-fps", "kits/fps", &[]),
        ];
        let violations = find_violations(
            &metadata(packages, &["engine-core", "kit-fps"])?,
            &config(json!([]))?,
        )?;
        assert!(violations.iter().any(|v| matches!(
            v,
            Violation::ZoneEdge {
                from,
                to_zone,
                ..
            } if from == "engine-core" && to_zone == "kits"
        )));
        Ok(())
    }

    #[test]
    fn kit_depending_on_game_is_zone_edge() -> TestResult {
        let packages = vec![
            package("kit-fps", "kits/fps", &[("game", true, None)]),
            package("game", "games/a/game", &[]),
        ];
        let violations = find_violations(
            &metadata(packages, &["kit-fps", "game"])?,
            &config(json!([]))?,
        )?;
        assert!(violations.iter().any(|v| matches!(
            v,
            Violation::ZoneEdge {
                from,
                to_zone,
                ..
            } if from == "kit-fps" && to_zone == "games"
        )));
        Ok(())
    }

    #[test]
    fn tool_depending_on_kit_is_zone_edge() -> TestResult {
        let packages = vec![
            package(
                "check-layers",
                "tools/check-layers",
                &[("kit-fps", true, None)],
            ),
            package("kit-fps", "kits/fps", &[]),
        ];
        let violations = find_violations(
            &metadata(packages, &["check-layers", "kit-fps"])?,
            &config(json!([]))?,
        )?;
        assert_eq!(
            violations,
            vec![Violation::ZoneEdge {
                from: "check-layers".into(),
                from_zone: "tools".into(),
                to: "kit-fps".into(),
                to_zone: "kits".into(),
                kind: "normal",
            }]
        );
        Ok(())
    }

    #[test]
    fn tool_depending_on_engine_is_clean() -> TestResult {
        let packages = vec![
            package(
                "check-layers",
                "tools/check-layers",
                &[("engine-core", true, None)],
            ),
            package("engine-core", "engine/core", &[]),
        ];
        let violations = find_violations(
            &metadata(packages, &["check-layers", "engine-core"])?,
            &config(json!([]))?,
        )?;
        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn same_game_mode_dependency_is_clean() -> TestResult {
        let packages = vec![
            package("game", "games/a/game", &[("mode-x", true, None)]),
            package("mode-x", "games/a/modes/x", &[]),
        ];
        let allowed = json!([{"from": "game", "to": "mode-x"}]);
        let violations =
            find_violations(&metadata(packages, &["game", "mode-x"])?, &config(allowed)?)?;
        assert!(violations.is_empty());
        Ok(())
    }

    #[test]
    fn cross_game_dependency_is_rejected() -> TestResult {
        let packages = vec![
            package("game-a", "games/a/game", &[("mode-b", true, None)]),
            package("mode-b", "games/b/modes/x", &[]),
        ];
        let allowed = json!([{"from": "game-a", "to": "mode-b"}]);
        let violations = find_violations(
            &metadata(packages, &["game-a", "mode-b"])?,
            &config(allowed)?,
        )?;
        assert_eq!(
            violations,
            vec![Violation::CrossGameEdge {
                from: "game-a".into(),
                from_game: "a".into(),
                to: "mode-b".into(),
                to_game: "b".into(),
                kind: "normal",
            }]
        );
        Ok(())
    }

    #[test]
    fn dev_dependency_crossing_zones_is_zone_edge() -> TestResult {
        let packages = vec![
            package(
                "engine-core",
                "engine/core",
                &[("kit-fps", true, Some("dev"))],
            ),
            package("kit-fps", "kits/fps", &[]),
        ];
        let violations = find_violations(
            &metadata(packages, &["engine-core", "kit-fps"])?,
            &config(json!([]))?,
        )?;
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, Violation::ZoneEdge { kind: "dev", .. }))
        );
        Ok(())
    }

    #[test]
    fn unmatched_crate_is_rejected() -> TestResult {
        let packages = vec![
            package("engine-core", "engine/core", &[]),
            package("stray", "misc/stray", &[("engine-core", true, None)]),
        ];
        let violations = find_violations(
            &metadata(packages, &["engine-core", "stray"])?,
            &config(json!([]))?,
        )?;
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, Violation::Unmatched { .. }))
        );
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, Violation::NoZone { .. }))
        );
        Ok(())
    }

    #[test]
    fn crate_matching_two_rules_is_ambiguous() -> TestResult {
        let config: LayerConfig = serde_json::from_value(json!({
            "rules": [
                {"pattern": "kits/*", "layer": 3},
                {"pattern": "kits/fps", "layer": 3}
            ],
            "zones": zones_json()
        }))?;
        let packages = vec![package("kit-fps", "kits/fps", &[])];
        let violations = find_violations(&metadata(packages, &["kit-fps"])?, &config)?;
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, Violation::Ambiguous { .. }))
        );
        Ok(())
    }

    #[test]
    fn crate_matching_two_zones_is_ambiguous_zone() -> TestResult {
        let config: LayerConfig = serde_json::from_value(json!({
            "rules": [{"pattern": "kits/*", "layer": 3}],
            "zones": [
                {"name": "kits", "patterns": ["kits/*"], "may_use": ["kits"], "scope": "shared"},
                {"name": "also", "patterns": ["kits/*"], "may_use": ["also"], "scope": "shared"}
            ]
        }))?;
        let packages = vec![package("kit-fps", "kits/fps", &[])];
        let violations = find_violations(&metadata(packages, &["kit-fps"])?, &config)?;
        assert_eq!(
            violations,
            vec![Violation::AmbiguousZone {
                package: "kit-fps".into(),
                dir: "kits/fps".into(),
                zones: "kits, also".into(),
            }]
        );
        Ok(())
    }

    #[test]
    fn same_layer_edge_needs_an_allowed_pair() -> TestResult {
        let packages = vec![
            package("engine-platform", "engine/platform", &[]),
            package(
                "engine-world",
                "engine/world",
                &[("engine-platform", true, None)],
            ),
        ];
        let members = ["engine-platform", "engine-world"];

        let rejected =
            find_violations(&metadata(packages.clone(), &members)?, &config(json!([]))?)?;
        assert_eq!(
            rejected,
            vec![Violation::SameLayerEdge {
                from: "engine-world".into(),
                to: "engine-platform".into(),
                layer: 1,
                kind: "normal",
            }]
        );

        let allowed = json!([{"from": "engine-world", "to": "engine-platform"}]);
        let accepted = find_violations(&metadata(packages, &members)?, &config(allowed)?)?;
        assert!(accepted.is_empty(), "unexpected: {accepted:?}");
        Ok(())
    }

    #[test]
    fn path_dependency_outside_workspace_is_rejected() -> TestResult {
        let packages = vec![
            package(
                "kit-fps",
                "kits/fps",
                &[("engine-spike-helper", true, None)],
            ),
            package("engine-spike-helper", "engine-spike/helper", &[]),
        ];
        let violations = find_violations(&metadata(packages, &["kit-fps"])?, &config(json!([]))?)?;
        assert_eq!(
            violations,
            vec![Violation::ExternalPath {
                from: "kit-fps".into(),
                to: "engine-spike-helper".into(),
                kind: "normal",
            }]
        );
        Ok(())
    }

    #[test]
    fn violation_messages_name_both_ends_of_the_edge() {
        let violation = Violation::UpwardEdge {
            from: "engine-core".into(),
            from_layer: 0,
            to: "engine-render".into(),
            to_layer: 2,
            kind: "normal",
        };
        let text = violation.to_string();
        assert!(text.contains("engine-core") && text.contains("engine-render"));
    }

    #[derive(Clone, Copy, Debug)]
    enum FixtureZone {
        Engine,
        Kits,
        Tools,
        Games,
    }

    fn zone_dir(zone: FixtureZone, game: &str, slot: u8) -> String {
        match zone {
            FixtureZone::Engine => format!("engine/c{slot}"),
            FixtureZone::Kits => format!("kits/k{slot}"),
            FixtureZone::Tools => format!("tools/t{slot}"),
            FixtureZone::Games => format!("games/{game}/game"),
        }
    }

    fn zone_name(zone: FixtureZone) -> &'static str {
        match zone {
            FixtureZone::Engine => "engine",
            FixtureZone::Kits => "kits",
            FixtureZone::Tools => "tools",
            FixtureZone::Games => "games",
        }
    }

    fn reference_zone_violations(
        config: &LayerConfig,
        dirs: &[(String, FixtureZone, String)],
        edges: &[(usize, usize, Option<&str>)],
    ) -> Vec<(String, String, String, String, &'static str)> {
        let mut out = Vec::new();
        for &(from, to, kind) in edges {
            let Some((from_name, from_zone, from_dir)) = dirs.get(from) else {
                continue;
            };
            let Some((to_name, to_zone, to_dir)) = dirs.get(to) else {
                continue;
            };
            let from_zn = zone_name(*from_zone);
            let to_zn = zone_name(*to_zone);
            let kind = dep_kind(kind);
            if !config.zone_allows_between(from_zn, to_zn) {
                out.push((
                    from_name.clone(),
                    from_zn.to_owned(),
                    to_name.clone(),
                    to_zn.to_owned(),
                    kind,
                ));
            } else if config.zone_is_per_game(from_zn)
                && config.zone_is_per_game(to_zn)
                && game_prefix(from_dir).is_some()
                && game_prefix(to_dir).is_some()
                && game_prefix(from_dir) != game_prefix(to_dir)
            {
                let (Some(from_game), Some(to_game)) = (game_prefix(from_dir), game_prefix(to_dir))
                else {
                    continue;
                };
                out.push((
                    from_name.clone(),
                    format!("cross:{from_game}"),
                    to_name.clone(),
                    format!("cross:{to_game}"),
                    kind,
                ));
            }
        }
        out
    }

    fn fixture_metadata(
        dirs: &[(String, FixtureZone, String)],
        edges: &[(usize, usize, Option<&str>)],
    ) -> Metadata {
        let packages: Vec<Value> = dirs
            .iter()
            .enumerate()
            .map(|(i, (name, _zone, dir))| {
                let deps: Vec<(&str, bool, Option<&str>)> = edges
                    .iter()
                    .filter(|(f, _, _)| *f == i)
                    .map(|(_, t, kind)| {
                        let dep = dirs.get(*t).expect("edge target").0.as_str();
                        (dep, true, *kind)
                    })
                    .collect();
                package(name, dir, &deps)
            })
            .collect();
        let members: Vec<&str> = dirs.iter().map(|(n, _, _)| n.as_str()).collect();
        metadata(packages, &members).expect("fixture metadata")
    }

    #[test]
    fn relative_dir_rejects_manifest_without_parent() {
        let root = Path::new(ROOT);
        let manifest = Path::new("");
        assert!(matches!(
            relative_dir(root, manifest),
            Err(CheckError::ManifestWithoutParent { .. })
        ));
    }

    #[test]
    fn relative_dir_rejects_path_outside_workspace() {
        let root = Path::new(ROOT);
        let manifest = Path::new("/other/crate/Cargo.toml");
        assert!(matches!(
            relative_dir(root, manifest),
            Err(CheckError::OutsideWorkspace { .. })
        ));
    }

    proptest! {
        #[test]
        fn zone_edges_match_reference(
            n in 2usize..8,
            edge_count in 0usize..16,
        ) {
            let config = config(json!([])).expect("fixture config");
            let mut dirs = Vec::new();
            for i in 0..n {
                let zone = match i % 4 {
                    0 => FixtureZone::Engine,
                    1 => FixtureZone::Kits,
                    2 => FixtureZone::Tools,
                    _ => FixtureZone::Games,
                };
                let game = if matches!(zone, FixtureZone::Games) {
                    if i % 2 == 0 { "ga" } else { "gb" }
                } else {
                    "x"
                };
                let slot = u8::try_from(i).expect("fixture slot");
                let dir = zone_dir(zone, game, slot);
                dirs.push((format!("p{i}"), zone, dir));
            }
            let mut edges = Vec::new();
            for e in 0..edge_count {
                let from = e % n;
                let to = (e * 3 + 1) % n;
                let kind = if e % 5 == 0 { Some("dev") } else { None };
                edges.push((from, to, kind));
            }
            let meta = fixture_metadata(&dirs, &edges);
            let violations = find_violations(&meta, &config).expect("zone judgement");
            let ref_edges = reference_zone_violations(&config, &dirs, &edges);
            let mut reported: Vec<_> = violations
                .iter()
                .filter_map(|v| match v {
                    Violation::ZoneEdge { from, from_zone, to, to_zone, kind } => {
                        Some((from.clone(), from_zone.clone(), to.clone(), to_zone.clone(), *kind))
                    }
                    Violation::CrossGameEdge { from, from_game, to, to_game, kind } => {
                        Some((
                            from.clone(),
                            format!("cross:{from_game}"),
                            to.clone(),
                            format!("cross:{to_game}"),
                            *kind,
                        ))
                    }
                    _ => None,
                })
                .collect();
            reported.sort();
            let mut expected = ref_edges;
            expected.sort();
            prop_assert_eq!(reported, expected);
        }
    }
}
