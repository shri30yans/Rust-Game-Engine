//! Reads crate source trees for forbidden substrings configured per zone.
//!
//! This is the only module besides [`crate::metadata`] that performs I/O.
//! Judgement of scanned text is pure ([`find_forbidden`]).

use std::fs;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::check::{CheckError, Violation};
use crate::rules::ZoneName;

/// One source file read from a crate directory, tagged with its zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Path of the file (absolute or relative to the workspace root).
    pub path: PathBuf,
    /// Full file text.
    pub content: String,
    /// Zone that owns the crate this file lives under.
    pub zone: ZoneName,
    /// Forbidden substrings for that zone.
    pub forbid_text: Vec<String>,
}

/// Recursively reads `*.rs`, `*.toml`, `*.wgsl` and `*.ron` under each crate
/// directory in `crates`, skipping any `target/` directory.
pub fn scan_zone_sources(
    root: &Path,
    crates: &[(String, ZoneName, Vec<String>)],
) -> Result<Vec<SourceFile>, Error> {
    let mut files = Vec::new();
    for (dir, zone, forbid_text) in crates {
        if forbid_text.is_empty() {
            continue;
        }
        let crate_root = root.join(dir);
        collect_sources(
            dir,
            &crate_root,
            &crate_root,
            zone.clone(),
            forbid_text.clone(),
            &mut files,
        )?;
    }
    Ok(files)
}

fn collect_sources(
    workspace_dir: &str,
    crate_root: &Path,
    current: &Path,
    zone: ZoneName,
    forbid_text: Vec<String>,
    out: &mut Vec<SourceFile>,
) -> Result<(), Error> {
    let entries = fs::read_dir(current).map_err(|source| Error::ReadSources {
        path: current.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::ReadSources {
            path: current.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if path.is_dir() {
            if name == "target" {
                continue;
            }
            collect_sources(
                workspace_dir,
                crate_root,
                &path,
                zone.clone(),
                forbid_text.clone(),
                out,
            )?;
            continue;
        }
        if !source_extension(&path) {
            continue;
        }
        let content = fs::read_to_string(&path).map_err(|source| Error::ReadSources {
            path: path.clone(),
            source,
        })?;
        let under_root = path_under_crate_root(&path, crate_root).map_err(Error::from)?;
        let rel = under_root
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let display = if workspace_dir.is_empty() {
            rel.clone()
        } else {
            format!("{workspace_dir}/{rel}")
        };
        out.push(SourceFile {
            path: PathBuf::from(display),
            content,
            zone: zone.clone(),
            forbid_text: forbid_text.clone(),
        });
    }
    Ok(())
}

/// Returns the portion of `path` below `crate_root`, or a layout error.
pub(crate) fn path_under_crate_root<'a>(
    path: &'a Path,
    crate_root: &Path,
) -> Result<&'a Path, CheckError> {
    path.strip_prefix(crate_root)
        .map_err(|_| CheckError::NotUnderCrateRoot {
            path: path.to_path_buf(),
            crate_root: crate_root.to_path_buf(),
        })
}

fn source_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("rs") | Some("toml") | Some("wgsl") | Some("ron")
    )
}

/// Reports every forbidden substring match with a 1-based line number.
pub fn find_forbidden(files: &[SourceFile]) -> Vec<Violation> {
    let mut violations = Vec::new();
    for file in files {
        for (index, line) in file.content.lines().enumerate() {
            let line_number = index + 1;
            for needle in &file.forbid_text {
                if line.contains(needle) {
                    violations.push(Violation::ForbiddenText {
                        file: file.path.display().to_string(),
                        line: line_number,
                        text: needle.clone(),
                        zone: file.zone.as_str().to_owned(),
                    });
                }
            }
        }
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use crate::check::Violation;
    use crate::rules::ZoneName;
    use serde_json::json;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(name);
            if path.exists() {
                std::fs::remove_dir_all(&path).expect("remove stale temp dir");
            }
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.path).expect("remove temp dir");
        }
    }

    fn zn(name: &str) -> ZoneName {
        serde_json::from_value(json!(name)).expect("zone name")
    }

    fn write_fixture(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        let mut file = std::fs::File::create(&path).expect("create");
        write!(file, "{body}").expect("write");
    }

    #[test]
    fn path_under_crate_root_rejects_outside_path() {
        let crate_root = Path::new("/ws/engine/core");
        let path = Path::new("/other/lib.rs");
        assert!(matches!(
            path_under_crate_root(path, crate_root),
            Err(CheckError::NotUnderCrateRoot { .. })
        ));
    }

    #[test]
    fn forbidden_text_is_reported_at_line_number() {
        let tmp = TempDir::new("check-layers-scan-forbidden");
        write_fixture(
            tmp.path(),
            "src/lib.rs",
            "fn ok() {}\nfn bad() {}\nconst P: &str = \"games/other/content\";\n",
        );
        let files = scan_zone_sources(
            tmp.path(),
            &[(String::new(), zn("engine"), vec!["games/".into()])],
        )
        .expect("scan");
        let violations = find_forbidden(&files);
        assert_eq!(
            violations,
            vec![Violation::ForbiddenText {
                file: "src/lib.rs".into(),
                line: 3,
                text: "games/".into(),
                zone: "engine".into(),
            }]
        );
    }

    #[test]
    fn tools_zone_without_forbid_text_skips_scan() {
        let tmp = TempDir::new("check-layers-scan-tools");
        write_fixture(
            tmp.path(),
            "src/main.rs",
            "const P: &str = \"games/other/content\";\n",
        );
        let files =
            scan_zone_sources(tmp.path(), &[(String::new(), zn("tools"), vec![])]).expect("scan");
        assert!(files.is_empty());
    }

    #[test]
    fn target_directory_is_skipped() {
        let tmp = TempDir::new("check-layers-scan-target");
        write_fixture(
            tmp.path(),
            "target/debug/out.rs",
            "const P: &str = \"games/\";\n",
        );
        write_fixture(tmp.path(), "src/lib.rs", "fn clean() {}\n");
        let files = scan_zone_sources(
            tmp.path(),
            &[(String::new(), zn("engine"), vec!["games/".into()])],
        )
        .expect("scan");
        assert_eq!(files.len(), 1);
        assert!(find_forbidden(&files).is_empty());
    }
}
