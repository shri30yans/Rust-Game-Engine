//! Versioned RON settings backed by the platform's game-agnostic text store.
//!
//! Missing files quietly use defaults. Read, parse, and version failures also use defaults, with
//! a warning for the caller to log. Writes occur only when the serialized settings value changes.

use std::fmt;

use platform::{ConfigName, ConfigStore, PlatformError};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Supplies the saved version and validates a value after deserialization.
pub trait VersionedSettings {
    /// The version accepted by this application.
    const VERSION: u32;

    /// Restores the type's invariants after decoding saved data.
    fn validate(&mut self);
}

/// A non-fatal issue encountered while loading a settings file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsWarning(String);

impl SettingsWarning {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SettingsWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Failures while encoding or writing settings.
#[derive(Debug, Error)]
pub enum SettingsFileError {
    /// RON could not encode the settings value.
    #[error("settings could not be serialized: {0}")]
    Serialize(#[from] ron::Error),
    /// The platform store could not write the settings file.
    #[error("settings could not be written: {0}")]
    Platform(#[from] PlatformError),
}

#[derive(Deserialize, Serialize)]
struct VersionedDocument<T> {
    version: u32,
    value: T,
}

/// Loads and saves a game-defined settings value using the platform config store.
pub struct SettingsFile<T>
where
    T: Serialize + DeserializeOwned + Default + PartialEq + Clone + VersionedSettings,
{
    store: ConfigStore,
    name: ConfigName,
    saved: T,
}

impl<T> SettingsFile<T>
where
    T: Serialize + DeserializeOwned + Default + PartialEq + Clone + VersionedSettings,
{
    /// Builds a settings file from a platform store and validated filename.
    pub fn new(store: ConfigStore, name: ConfigName) -> Self {
        Self {
            store,
            name,
            saved: T::default(),
        }
    }

    /// Loads settings and returns a non-fatal warning when storage or decoding fails.
    pub fn load(&mut self) -> (T, Option<SettingsWarning>) {
        match self.store.read(&self.name) {
            Ok(None) => self.defaults(None),
            Ok(Some(text)) => match self.decode(&text) {
                Ok(value) => {
                    self.saved = value.clone();
                    (value, None)
                }
                Err(warning) => self.defaults(Some(warning)),
            },
            Err(error) => self.defaults(Some(SettingsWarning::new(format!(
                "settings could not be read: {error}"
            )))),
        }
    }

    /// Writes `value` only when it differs from the last loaded or saved value.
    pub fn save_if_changed(&mut self, value: &T) -> Result<(), SettingsFileError> {
        if value == &self.saved {
            return Ok(());
        }

        let text = ron::ser::to_string(&VersionedDocument {
            version: T::VERSION,
            value,
        })?;
        self.store.write(&self.name, &text)?;
        self.saved = value.clone();
        Ok(())
    }

    fn defaults(&mut self, warning: Option<SettingsWarning>) -> (T, Option<SettingsWarning>) {
        let defaults = T::default();
        self.saved = defaults.clone();
        (defaults, warning)
    }

    fn decode(&self, text: &str) -> Result<T, SettingsWarning> {
        let mut document = ron::from_str::<VersionedDocument<T>>(text).map_err(|error| {
            SettingsWarning::new(format!("settings could not be decoded: {error}"))
        })?;
        if document.version != T::VERSION {
            return Err(SettingsWarning::new(format!(
                "settings version {} is not supported",
                document.version
            )));
        }
        document.value.validate();
        Ok(document.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
    struct TestSettings {
        value: u8,
    }

    impl VersionedSettings for TestSettings {
        const VERSION: u32 = 3;

        fn validate(&mut self) {
            self.value = self.value.min(100);
        }
    }

    fn test_file() -> Result<(SettingsFile<TestSettings>, PathBuf), Box<dyn std::error::Error>> {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("engine-app-settings-{}-{id}", std::process::id()));
        fs::create_dir_all(&root)?;
        let file = SettingsFile::new(
            ConfigStore::from_root(&root),
            ConfigName::new("settings.ron")?,
        );
        Ok((file, root))
    }

    #[test]
    fn missing_file_is_silent_and_defaults_are_not_written()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut file, root) = test_file()?;
        let (settings, warning) = file.load();
        assert_eq!(settings, TestSettings::default());
        assert!(warning.is_none());
        file.save_if_changed(&settings)?;
        assert!(!root.join("settings.ron").exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn saved_values_round_trip_and_unchanged_values_are_not_rewritten()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut file, root) = test_file()?;
        let (mut settings, _) = file.load();
        settings.value = 42;
        file.save_if_changed(&settings)?;
        let saved = fs::read_to_string(root.join("settings.ron"))?;
        file.save_if_changed(&settings)?;
        assert_eq!(fs::read_to_string(root.join("settings.ron"))?, saved);
        let (loaded, warning) = file.load();
        assert_eq!(loaded, settings);
        assert!(warning.is_none());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn invalid_and_newer_files_fall_back_with_a_warning() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut file, root) = test_file()?;
        let path = root.join("settings.ron");
        fs::write(&path, "not RON")?;
        let (settings, warning) = file.load();
        assert_eq!(settings, TestSettings::default());
        assert!(warning.is_some());
        fs::write(&path, "(version:4,value:(value:40))")?;
        let (settings, warning) = file.load();
        assert_eq!(settings, TestSettings::default());
        assert!(warning.is_some());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn unreadable_file_falls_back_with_a_warning() -> Result<(), Box<dyn std::error::Error>> {
        let (mut file, root) = test_file()?;
        fs::create_dir(root.join("settings.ron"))?;
        let (settings, warning) = file.load();
        assert_eq!(settings, TestSettings::default());
        assert!(warning.is_some());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn values_are_validated_after_loading() -> Result<(), Box<dyn std::error::Error>> {
        let (mut file, root) = test_file()?;
        fs::write(root.join("settings.ron"), "(version:3,value:(value:255))")?;
        let (settings, warning) = file.load();
        assert_eq!(settings.value, 100);
        assert!(warning.is_none());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_text_never_breaks_load(text in ".*") {
            let (mut file, root) = test_file().map_err(|error| proptest::test_runner::TestCaseError::fail(error.to_string()))?;
            fs::write(root.join("settings.ron"), text)
                .map_err(|error| proptest::test_runner::TestCaseError::fail(error.to_string()))?;
            let (settings, _) = file.load();
            proptest::prop_assert!(settings.value <= 100);
            fs::remove_dir_all(root)
                .map_err(|error| proptest::test_runner::TestCaseError::fail(error.to_string()))?;
        }
    }
}
