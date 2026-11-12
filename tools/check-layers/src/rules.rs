//! Layer table and folder zones, parsed from `tools/check-layers/layers.json`.
//!
//! Layer rules map workspace-relative directory globs to a layer number; zone
//! rules map the same globs to decoupling policy. In a glob, `*` matches exactly one path segment.
//!
//! `same_layer_allowed` lists the only same-layer dependencies that are
//! permitted, as directed `from` -> `to` pairs of package names.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

/// The parsed layer table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerConfig {
    /// Directory-glob rules that assign each workspace crate to a layer.
    pub rules: Vec<LayerRule>,
    /// Folder zones and their dependency policy.
    #[serde(default)]
    pub zones: Vec<Zone>,
    /// Same-layer dependencies that are allowed, by package name.
    #[serde(default)]
    pub same_layer_allowed: Vec<AllowedPair>,
}

/// Assigns every crate whose workspace-relative directory matches `pattern`
/// to `layer`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerRule {
    /// Workspace-relative directory glob, segments separated by `/`.
    pub pattern: String,
    /// Layer number; lower layers are more fundamental.
    pub layer: u32,
}

/// A platform folder zone: which directories belong to it and which other
/// zones its crates may depend on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    /// Stable zone identifier.
    pub name: ZoneName,
    /// Directory globs for crates in this zone (same syntax as layer rules).
    pub patterns: Vec<String>,
    /// Zone names this zone's crates may depend on (must include `name` for same-zone edges).
    pub may_use: Vec<ZoneName>,
    /// Whether dependencies inside the zone must stay under one `games/<name>` tree.
    pub scope: ZoneScope,
    /// Substrings that must not appear in this zone's source files.
    #[serde(default)]
    pub forbid_text: Vec<String>,
}

/// Non-empty zone identifier from `layers.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(transparent)]
pub struct ZoneName(String);

impl ZoneName {
    /// Borrows the inner name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ZoneName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether crates in a zone share one tree or are partitioned per game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneScope {
    /// All crates in the zone share one dependency policy.
    Shared,
    /// Crates under `games/<name>/` may depend only on paths in the same game.
    PerGame,
}

/// A permitted same-layer dependency: `from` may depend on `to`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowedPair {
    /// Package name of the depending crate.
    pub from: String,
    /// Package name of the crate being depended on.
    pub to: String,
}

/// Invalid zone or layer table contents after JSON parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// Two zones share the same name.
    #[error("duplicate zone name `{0}`")]
    DuplicateZoneName(String),
    /// `may_use` names a zone that does not exist.
    #[error("zone `{zone}` lists unknown zone `{unknown}` in may_use")]
    UnknownMayUseZone {
        /// Zone whose `may_use` list is invalid.
        zone: String,
        /// Name that does not match any zone.
        unknown: String,
    },
    /// A zone has no patterns.
    #[error("zone `{0}` has no patterns")]
    EmptyPatterns(String),
    /// A zone lists an empty forbidden substring.
    #[error("zone `{zone}` has an empty forbid_text entry")]
    EmptyForbidText {
        /// Zone with the invalid entry.
        zone: String,
    },
}

impl LayerConfig {
    /// Parses a layer table from JSON text.
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Checks zone names, `may_use` targets, patterns and `forbid_text` entries.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut seen = HashSet::new();
        let known: HashMap<&str, ()> = self
            .zones
            .iter()
            .map(|zone| (zone.name.as_str(), ()))
            .collect();

        for zone in &self.zones {
            if !seen.insert(zone.name.as_str()) {
                return Err(ConfigError::DuplicateZoneName(zone.name.to_string()));
            }
            if zone.patterns.is_empty() {
                return Err(ConfigError::EmptyPatterns(zone.name.to_string()));
            }
            for entry in &zone.forbid_text {
                if entry.is_empty() {
                    return Err(ConfigError::EmptyForbidText {
                        zone: zone.name.to_string(),
                    });
                }
            }
            for target in &zone.may_use {
                if !known.contains_key(target.as_str()) {
                    return Err(ConfigError::UnknownMayUseZone {
                        zone: zone.name.to_string(),
                        unknown: target.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Returns every rule whose glob matches the workspace-relative `dir`.
    pub fn matching_rules(&self, dir: &str) -> Vec<&LayerRule> {
        self.rules
            .iter()
            .filter(|rule| glob_matches(&rule.pattern, dir))
            .collect()
    }

    /// Returns every zone whose patterns match the workspace-relative `dir`.
    pub fn matching_zones(&self, dir: &str) -> Vec<&Zone> {
        self.zones
            .iter()
            .filter(|zone| zone.patterns.iter().any(|pat| glob_matches(pat, dir)))
            .collect()
    }

    /// Reports whether package `from` may depend on package `to` within one layer.
    pub fn allows_same_layer(&self, from: &str, to: &str) -> bool {
        self.same_layer_allowed
            .iter()
            .any(|pair| pair.from == from && pair.to == to)
    }

    /// Reports whether a crate in `from_zone` may depend on a crate in `to_zone`.
    pub fn zone_allows_edge(&self, from_zone: &ZoneName, to_zone: &ZoneName) -> bool {
        self.zone_allows_between(from_zone.as_str(), to_zone.as_str())
    }

    /// Like [`zone_allows_edge`](Self::zone_allows_edge) but takes zone name strings.
    pub fn zone_allows_between(&self, from_zone: &str, to_zone: &str) -> bool {
        let Some(from) = self
            .zones
            .iter()
            .find(|zone| zone.name.as_str() == from_zone)
        else {
            return false;
        };
        from.may_use
            .iter()
            .any(|allowed| allowed.as_str() == to_zone)
    }

    /// Returns `true` when `zone` uses [`ZoneScope::PerGame`].
    pub fn zone_is_per_game(&self, zone: &str) -> bool {
        self.zones
            .iter()
            .find(|entry| entry.name.as_str() == zone)
            .is_some_and(|entry| entry.scope == ZoneScope::PerGame)
    }

    /// Looks up a zone by name.
    pub fn zone_by_name(&self, name: &ZoneName) -> Option<&Zone> {
        self.zones.iter().find(|zone| &zone.name == name)
    }
}

/// Matches a `/`-separated directory against a glob where `*` stands for
/// exactly one segment. Segment counts must be equal.
pub fn glob_matches(pattern: &str, dir: &str) -> bool {
    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let dir_segments: Vec<&str> = dir.split('/').collect();
    pattern_segments.len() == dir_segments.len()
        && pattern_segments
            .iter()
            .zip(dir_segments.iter())
            .all(|(pat, seg)| *pat == "*" || pat == seg)
}

/// Returns `games/<name>` when `dir` starts with `games/`, otherwise `None`.
pub fn game_prefix(dir: &str) -> Option<&str> {
    let mut segments = dir.split('/');
    let games = segments.next()?;
    if games != "games" {
        return None;
    }
    let name = segments.next()?;
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn zn(name: &str) -> Result<ZoneName, serde_json::Error> {
        serde_json::from_value(json!(name))
    }

    #[test]
    fn star_matches_exactly_one_segment() {
        assert!(glob_matches("multiplayer/*", "multiplayer/rooms"));
        assert!(!glob_matches("multiplayer/*", "multiplayer"));
        assert!(!glob_matches("multiplayer/*", "multiplayer/rooms/extra"));
        assert!(!glob_matches("multiplayer/*", "kits/rooms"));
    }

    #[test]
    fn literal_patterns_match_exactly() {
        assert!(glob_matches("engine/core", "engine/core"));
        assert!(!glob_matches("engine/core", "engine/core2"));
        assert!(!glob_matches("engine/core", "engine"));
    }

    #[test]
    fn unknown_field_in_layer_table_is_rejected() {
        let text = r#"{"rules":[{"pattern":"a","layer":0,"layr":1}]}"#;
        assert!(LayerConfig::parse(text).is_err());
    }

    #[test]
    fn same_layer_allowance_is_directional() -> TestResult {
        let config: LayerConfig = serde_json::from_value(json!({
            "rules": [],
            "same_layer_allowed": [{"from": "a", "to": "b"}]
        }))?;
        assert!(config.allows_same_layer("a", "b"));
        assert!(!config.allows_same_layer("b", "a"));
        Ok(())
    }

    fn sample_zones() -> Result<Vec<Zone>, serde_json::Error> {
        serde_json::from_value(json!([
            { "name": "engine", "patterns": ["engine/*"], "may_use": ["engine"], "scope": "shared", "forbid_text": ["games/"] },
            { "name": "kits", "patterns": ["kits/*"], "may_use": ["engine", "kits"], "scope": "shared", "forbid_text": ["games/"] },
            { "name": "tools", "patterns": ["tools/*"], "may_use": ["engine", "tools"], "scope": "shared" },
            { "name": "games", "patterns": ["games/*/game", "games/*/modes/*"], "may_use": ["engine", "kits", "games"], "scope": "per_game" }
        ]))
    }

    fn zone_a(name: &str, patterns: &[&str], may_use: &[&str]) -> Result<Zone, serde_json::Error> {
        let patterns: Vec<String> = patterns.iter().map(|p| (*p).to_owned()).collect();
        let may_use: Vec<ZoneName> = may_use
            .iter()
            .map(|z| zn(z))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Zone {
            name: zn(name)?,
            patterns,
            may_use,
            scope: ZoneScope::Shared,
            forbid_text: vec![],
        })
    }

    #[test]
    fn validate_rejects_duplicate_zone_name() -> TestResult {
        let one = zone_a("a", &["x/*"], &["a"])?;
        let two = Zone {
            patterns: vec!["y/*".into()],
            ..zone_a("a", &["y/*"], &["a"])?
        };
        let config = LayerConfig {
            rules: vec![],
            zones: vec![one, two],
            same_layer_allowed: vec![],
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::DuplicateZoneName(_))
        ));
        Ok(())
    }

    #[test]
    fn validate_rejects_unknown_may_use() -> TestResult {
        let config = LayerConfig {
            rules: vec![],
            zones: vec![zone_a("a", &["x/*"], &["missing"])?],
            same_layer_allowed: vec![],
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::UnknownMayUseZone { .. })
        ));
        Ok(())
    }

    #[test]
    fn validate_rejects_empty_patterns() -> TestResult {
        let config = LayerConfig {
            rules: vec![],
            zones: vec![Zone {
                patterns: vec![],
                ..zone_a("a", &["x/*"], &["a"])?
            }],
            same_layer_allowed: vec![],
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::EmptyPatterns(_))
        ));
        Ok(())
    }

    #[test]
    fn validate_rejects_empty_forbid_text() -> TestResult {
        let config = LayerConfig {
            rules: vec![],
            zones: vec![Zone {
                forbid_text: vec!["".into()],
                ..zone_a("a", &["x/*"], &["a"])?
            }],
            same_layer_allowed: vec![],
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::EmptyForbidText { .. })
        ));
        Ok(())
    }

    #[test]
    fn validate_accepts_sample_zones() -> TestResult {
        let config = LayerConfig {
            rules: vec![],
            zones: sample_zones()?,
            same_layer_allowed: vec![],
        };
        assert!(config.validate().is_ok());
        Ok(())
    }
}
