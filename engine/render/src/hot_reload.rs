//! Dev-only file watching for WGSL hot reload.
//!
//! Each shader file is watched with [`engine_core::watch::MtimeWatch`], the same throttled mtime
//! poll that level hot reload uses.

use crate::RenderError;
use crate::shaders::{ShaderId, shader_dir};
use engine_core::watch::MtimeWatch;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Fixed-capacity list of changed shader ids (no heap allocation).
#[derive(Debug)]
pub(crate) struct SmallList {
    ids: [ShaderId; ShaderId::ALL.len()],
    len: usize,
}

impl SmallList {
    pub(crate) fn new() -> Self {
        Self {
            ids: [ShaderId::Scene; ShaderId::ALL.len()],
            len: 0,
        }
    }

    pub(crate) fn push(&mut self, id: ShaderId) {
        if self.len >= self.ids.len() || self.contains(id) {
            return;
        }
        if let Some(slot) = self.ids.get_mut(self.len) {
            *slot = id;
            self.len += 1;
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = ShaderId> + '_ {
        self.ids.iter().take(self.len).copied()
    }

    fn contains(&self, id: ShaderId) -> bool {
        self.iter().any(|x| x == id)
    }
}

/// Watches shader files on disk and reports which modules changed.
pub(crate) struct ShaderWatcher {
    root: PathBuf,
    files: HashMap<String, MtimeWatch>,
}

impl ShaderWatcher {
    pub(crate) fn new(root: &Path) -> Result<Self, RenderError> {
        let mut files = HashMap::new();
        let epoch = Instant::now();
        for id in ShaderId::ALL {
            for &name in id.files() {
                let path = root.join(name);
                let watch = MtimeWatch::new(path.clone(), epoch)
                    .map_err(|source| RenderError::ShaderWatch { path, source })?;
                files.insert(name.to_owned(), watch);
            }
        }
        Ok(Self {
            root: root.to_path_buf(),
            files,
        })
    }

    pub(crate) fn default_watcher() -> Result<Self, RenderError> {
        Self::new(&shader_dir())
    }

    /// Shader ids whose backing files changed since the last successful observation.
    pub(crate) fn poll(&mut self, now: Instant) -> Result<SmallList, RenderError> {
        let mut out = SmallList::new();
        let mut touched = HashMap::<ShaderId, ()>::new();
        for id in ShaderId::ALL {
            for &name in id.files() {
                let Some(state) = self.files.get_mut(name) else {
                    continue;
                };
                let changed = state.poll(now).map_err(|source| RenderError::ShaderWatch {
                    path: self.root.join(name),
                    source,
                })?;
                if changed {
                    touched.entry(id).or_insert(());
                }
            }
        }
        for id in ShaderId::ALL {
            if touched.contains_key(&id) {
                out.push(id);
            }
        }
        Ok(out)
    }
}

#[cfg(all(test, feature = "hot-reload"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use engine_core::watch::POLL_INTERVAL;
    use std::fs::File;
    use std::io::Write;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn write_file(dir: &Path, name: &str, body: &str) {
        let path = dir.join(name);
        let mut f = File::create(path).expect("create");
        f.write_all(body.as_bytes()).expect("write");
    }

    fn write_all_shader_stubs(dir: &Path) {
        for id in ShaderId::ALL {
            for &name in id.files() {
                write_file(dir, name, "// stub");
            }
        }
    }

    fn reset_dir(dir: &Path) {
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
        std::fs::create_dir_all(dir).unwrap();
    }

    fn touch(dir: &Path, name: &str, when: SystemTime) {
        let path = dir.join(name);
        File::open(&path)
            .expect("open")
            .set_modified(when)
            .expect("set_modified");
    }

    #[test]
    fn new_missing_shader_file_returns_shader_watch() {
        let dir = std::env::temp_dir().join(format!("shader-watch-new-{}", std::process::id()));
        reset_dir(&dir);
        write_file(&dir, "post.wgsl", "// only one file");

        assert!(matches!(
            ShaderWatcher::new(&dir),
            Err(RenderError::ShaderWatch { .. })
        ));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn poll_detects_changes() {
        let dir = std::env::temp_dir().join(format!("shader-watch-{}", std::process::id()));
        reset_dir(&dir);
        write_all_shader_stubs(&dir);

        let mut w = ShaderWatcher::new(&dir).expect("new");
        let t0 = Instant::now();
        assert!(w.poll(t0).unwrap().iter().next().is_none());

        touch(&dir, "post.wgsl", UNIX_EPOCH + Duration::from_secs(10));
        let t1 = t0 + POLL_INTERVAL;
        let changed: Vec<_> = w.poll(t1).unwrap().iter().collect();
        assert_eq!(changed, [ShaderId::Post]);

        touch(
            &dir,
            "scene_lighting.wgsl",
            UNIX_EPOCH + Duration::from_secs(20),
        );
        let t2 = t1 + POLL_INTERVAL;
        let changed: Vec<_> = w.poll(t2).unwrap().iter().collect();
        assert_eq!(changed, [ShaderId::Scene]);

        let t2_fast = t1 + Duration::from_millis(50);
        assert!(
            w.poll(t2_fast).unwrap().iter().next().is_none(),
            "poll inside throttle window"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn poll_missing_file_returns_shader_watch() {
        let dir = std::env::temp_dir().join(format!("shader-watch-del-{}", std::process::id()));
        reset_dir(&dir);
        write_all_shader_stubs(&dir);

        let mut w = ShaderWatcher::new(&dir).expect("new");
        let t0 = Instant::now();
        assert!(w.poll(t0).unwrap().iter().next().is_none());

        std::fs::remove_file(dir.join("post.wgsl")).expect("remove");
        let t1 = t0 + POLL_INTERVAL;
        let err = w.poll(t1).unwrap_err();
        assert!(matches!(err, RenderError::ShaderWatch { .. }));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
