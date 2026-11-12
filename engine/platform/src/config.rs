//! Portable, game-agnostic text configuration storage. Names are validated before joining paths,
//! and writes replace a file through a sibling temporary file.

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use std::env;
use std::fs;
#[cfg(target_os = "android")]
use std::path::Path;
use std::path::PathBuf;

use crate::PlatformError;

const MAX_APP_ID_LEN: usize = 64;
const MAX_CONFIG_NAME_LEN: usize = 128;

/// A validated application identifier used as one directory name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppId(String);

impl AppId {
    /// Validates and stores an ASCII identifier of 1 to 64 letters, digits, `-` or `_`.
    pub fn new(value: impl Into<String>) -> Result<Self, PlatformError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_APP_ID_LEN
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(invalid_config("app id", value));
        }
        Ok(Self(value))
    }

    /// The validated identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated plain filename within an application config directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigName(String);

impl ConfigName {
    /// Validates a portable filename with no path separators or traversal component.
    pub fn new(value: impl Into<String>) -> Result<Self, PlatformError> {
        let value = value.into();
        if value.is_empty()
            || value == "."
            || value.len() > MAX_CONFIG_NAME_LEN
            || value.contains("..")
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(invalid_config("filename", value));
        }
        Ok(Self(value))
    }

    /// The validated filename text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stores arbitrary text files under one application's platform config directory.
#[derive(Clone, Debug)]
pub struct ConfigStore {
    root: PathBuf,
}

impl ConfigStore {
    /// Resolves the platform config directory once and appends `app_id`.
    pub fn new(app_id: AppId) -> Result<Self, PlatformError> {
        let root = platform_config_root()?.join(app_id.as_str());
        Ok(Self { root })
    }

    /// Uses an explicit root directory, for tests and headless applications.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Reads a file, returning `None` when it does not exist.
    pub fn read(&self, name: &ConfigName) -> Result<Option<String>, PlatformError> {
        let path = self.root.join(&name.0);
        match fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Atomically writes text through a sibling `.tmp` file, creating the config directory first.
    pub fn write(&self, name: &ConfigName, text: &str) -> Result<(), PlatformError> {
        fs::create_dir_all(&self.root)?;
        let path = self.root.join(&name.0);
        let temporary = self.root.join(format!("{}.tmp", name.0));
        fs::write(&temporary, text)?;
        fs::rename(temporary, path)?;
        Ok(())
    }
}

fn invalid_config(kind: &'static str, value: String) -> PlatformError {
    PlatformError::InvalidConfigName { kind, value }
}

fn platform_config_root() -> Result<PathBuf, PlatformError> {
    #[cfg(target_os = "android")]
    {
        Err(PlatformError::ConfigDirectoryUnavailable)
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().map(|home| home.join("Library/Application Support"))
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(path) = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
            if path.is_absolute() {
                return Ok(path);
            }
        }
        home_dir().map(|home| home.join(".config"))
    }
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or(PlatformError::ConfigDirectoryUnavailable)
    }
    #[cfg(not(any(
        target_os = "android",
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    )))]
    {
        Err(PlatformError::ConfigDirectoryUnavailable)
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn home_dir() -> Result<PathBuf, PlatformError> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or(PlatformError::ConfigDirectoryUnavailable)
}

/// Creates a store rooted in an Android activity's internal data directory.
#[cfg(target_os = "android")]
pub fn from_android_internal_path(app_id: AppId, internal_data_path: &Path) -> ConfigStore {
    ConfigStore::from_root(internal_data_path.join(app_id.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_store() -> Result<(ConfigStore, PathBuf), Box<dyn std::error::Error>> {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!(
            "engine-platform-config-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok((ConfigStore::from_root(&root), root))
    }

    #[test]
    fn config_write_and_read_round_trip_without_a_temp_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let (store, root) = temp_store()?;
        let name = ConfigName::new("settings.ron")?;
        assert_eq!(store.read(&name)?, None);
        store.write(&name, "(value: 3)")?;
        assert_eq!(store.read(&name)?.as_deref(), Some("(value: 3)"));
        assert!(!root.join("settings.ron.tmp").exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn app_ids_reject_invalid_values() {
        for value in ["", "has space", "bad/part", &"x".repeat(65)] {
            assert!(AppId::new(value).is_err(), "{value:?}");
        }
        assert!(AppId::new("GAME_1").is_ok());
    }

    proptest::proptest! {
        #[test]
        fn config_names_reject_path_syntax(value in proptest::prop_oneof![
            proptest::strategy::Just(String::new()),
            "[a-z]{1,10}/[a-z]{0,10}",
            "[a-z]{1,10}\\\\[a-z]{0,10}",
            "[a-z]{0,10}\\.\\.[a-z]{0,10}",
            ".{129,140}",
        ]) {
            proptest::prop_assert!(ConfigName::new(value).is_err());
        }
    }

    #[test]
    fn config_names_reject_empty_separator_and_traversal_values() {
        for value in ["", "a/b", "a\\b", "..", "a..b", &"x".repeat(129)] {
            assert!(ConfigName::new(value).is_err(), "{value:?}");
        }
    }
}
