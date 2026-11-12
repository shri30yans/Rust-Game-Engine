//! Dev-only modification-time polling used by shader hot reload.
//!
//! [`MtimeWatch`] reads one file's metadata at most once per [`POLL_INTERVAL`]. The clock is an
//! argument, so tests drive it without sleeping. This module is the only place engine-core reads
//! the file system, and it only reads metadata.
//! Invariants: a poll inside the throttle window does not touch the disk; a file is "changed" only
//! when its modification time differs from the last observed one.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Minimum interval between metadata reads of one file.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// One file whose modification time is polled for changes.
#[derive(Debug)]
pub struct MtimeWatch {
    path: PathBuf,
    mtime: SystemTime,
    /// When the file was last read. `None` until the first poll, which always reads.
    last_checked: Option<Instant>,
}

impl MtimeWatch {
    /// Starts watching `path`, recording its current modification time as observed at `now`.
    ///
    /// # Errors
    ///
    /// Returns the metadata error when the file cannot be read.
    pub fn new(path: PathBuf, now: Instant) -> io::Result<Self> {
        let mtime = mtime_of(&path)?;
        Ok(Self {
            path,
            mtime,
            last_checked: Some(now),
        })
    }

    /// Starts watching `path` without a clock. The first poll always reads the file, so a change
    /// made between this call and that poll is seen. Use this where no clock is available; the
    /// throttle starts with the first poll.
    ///
    /// # Errors
    ///
    /// Returns the metadata error when the file cannot be read.
    pub fn start(path: PathBuf) -> io::Result<Self> {
        let mtime = mtime_of(&path)?;
        Ok(Self {
            path,
            mtime,
            last_checked: None,
        })
    }

    /// The watched path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns `true` when the file changed since the last observation. A poll closer than
    /// [`POLL_INTERVAL`] to the previous read returns `false` without reading the file.
    ///
    /// # Errors
    ///
    /// Returns the metadata error when the file is due for a read and cannot be read (for
    /// example it was deleted). The previous observation is kept.
    pub fn poll(&mut self, now: Instant) -> io::Result<bool> {
        if let Some(last) = self.last_checked
            && now.duration_since(last) < POLL_INTERVAL
        {
            return Ok(false);
        }
        self.last_checked = Some(now);
        let mtime = mtime_of(&self.path)?;
        if mtime == self.mtime {
            return Ok(false);
        }
        self.mtime = mtime;
        Ok(true)
    }
}

fn mtime_of(path: &Path) -> io::Result<SystemTime> {
    std::fs::metadata(path)?.modified()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::time::UNIX_EPOCH;

    fn temp_file(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("core-watch-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("watched.txt");
        std::fs::write(&path, "one").unwrap();
        path
    }

    fn set_mtime(path: &Path, secs: u64) {
        File::open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn reports_a_change_only_after_the_modification_time_moves() {
        let path = temp_file("change");
        set_mtime(&path, 10);
        let t0 = Instant::now();
        let mut watch = MtimeWatch::new(path.clone(), t0).unwrap();
        let later = t0 + POLL_INTERVAL;
        assert!(!watch.poll(later).unwrap());
        set_mtime(&path, 20);
        let later2 = later + POLL_INTERVAL;
        assert!(watch.poll(later2).unwrap());
        assert!(!watch.poll(later2 + POLL_INTERVAL).unwrap());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_poll_inside_the_window_does_not_read_the_file() {
        let path = temp_file("window");
        set_mtime(&path, 10);
        let t0 = Instant::now();
        let mut watch = MtimeWatch::new(path.clone(), t0).unwrap();
        set_mtime(&path, 20);
        assert!(!watch.poll(t0 + Duration::from_millis(10)).unwrap());
        assert!(watch.poll(t0 + POLL_INTERVAL).unwrap());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_missing_file_is_an_error_and_keeps_the_observation() {
        let path = temp_file("missing");
        set_mtime(&path, 10);
        let t0 = Instant::now();
        let mut watch = MtimeWatch::new(path.clone(), t0).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(watch.poll(t0 + POLL_INTERVAL).is_err());
        std::fs::write(&path, "back").unwrap();
        set_mtime(&path, 30);
        assert!(watch.poll(t0 + POLL_INTERVAL * 2).unwrap());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_started_watch_reads_the_file_at_its_first_poll() {
        let path = temp_file("started");
        set_mtime(&path, 10);
        let mut watch = MtimeWatch::start(path.clone()).unwrap();
        set_mtime(&path, 20);
        assert!(watch.poll(Instant::now()).unwrap());
        assert!(!watch.poll(Instant::now()).unwrap());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn new_on_a_missing_file_is_an_error() {
        let dir = std::env::temp_dir().join(format!("core-watch-none-{}", std::process::id()));
        assert!(MtimeWatch::new(dir.join("absent"), Instant::now()).is_err());
    }
}
