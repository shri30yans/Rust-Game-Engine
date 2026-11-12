//! Opens the one game pack through the OS content source and owns platform startup context.
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use engine_core::{content_pack_filename, game_content_pack_path};

use crate::{DeviceClass, DeviceProfile, PlatformError};

/// A stream opened from the operating system's content source.
pub trait ContentStream: Read + Seek {}

impl<T: Read + Seek> ContentStream for T {}

/// Desktop pack metadata used to notice development cook output changes without reading the pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRevision {
    /// Last filesystem modification time.
    pub modified: SystemTime,
    /// Pack length in bytes.
    pub size: u64,
}

/// Platform startup context used to detect the device, open game content and run its window.
pub struct ContentSource {
    #[cfg(target_os = "android")]
    android: Option<crate::AndroidApp>,
}

impl ContentSource {
    /// Creates a content source for headless execution using the platform's development lookup.
    #[must_use]
    pub const fn headless() -> Self {
        #[cfg(target_os = "android")]
        return Self { android: None };
        #[cfg(not(target_os = "android"))]
        Self::desktop()
    }

    /// Creates the desktop content source.
    #[cfg(not(target_os = "android"))]
    #[must_use]
    pub const fn desktop() -> Self {
        Self {}
    }

    /// Creates an Android content source bound to the NativeActivity.
    #[cfg(target_os = "android")]
    #[must_use]
    pub const fn android(android: crate::AndroidApp) -> Self {
        Self {
            android: Some(android),
        }
    }

    /// Opens the game's `.pak` beside the executable, from its canonical development path, or
    /// from Android APK assets.
    pub fn open_content(&self, name: &str) -> Result<Box<dyn ContentStream>, PlatformError> {
        #[cfg(target_os = "android")]
        if let Some(android) = self.android.as_ref() {
            let filename = std::ffi::CString::new(content_pack_filename(name)).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "content pack name contains a null byte",
                )
            })?;
            let asset = android.asset_manager().open(&filename).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("APK asset `{}` is missing", filename.to_string_lossy()),
                )
            })?;
            return Ok(Box::new(asset));
        }
        Ok(Box::new(File::open(self.content_path(name)?)?))
    }

    /// Returns a development pack revision when the platform source can change.
    pub fn content_revision(&self, name: &str) -> Result<Option<ContentRevision>, PlatformError> {
        #[cfg(target_os = "android")]
        if self.android.is_some() {
            return Ok(None);
        }
        let metadata = std::fs::metadata(self.content_path(name)?)?;
        Ok(Some(ContentRevision {
            modified: metadata.modified()?,
            size: metadata.len(),
        }))
    }

    fn content_path(&self, name: &str) -> Result<PathBuf, PlatformError> {
        let executable = std::env::current_exe()?;
        let executable_root = executable.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "executable directory unavailable",
            )
        })?;
        let installed = executable_root.join(content_pack_filename(name));
        if installed.is_file() {
            return Ok(installed);
        }
        let working_root = std::env::current_dir()?;
        for search_root in working_root.ancestors() {
            let direct = game_content_pack_path(search_root, name);
            if direct.is_file() {
                return Ok(direct);
            }
            if search_root.join("Cargo.toml").is_file()
                && let Some(development) = find_development_pack(search_root, name)?
            {
                return Ok(development);
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "content pack `{}` not found beside the executable or in the development workspace",
                content_pack_filename(name)
            ),
        )
        .into())
    }

    /// Detects the platform class and whether touch controls should be active.
    pub fn device_profile(&self) -> Result<DeviceProfile, PlatformError> {
        #[cfg(target_os = "android")]
        let class = if self.android.is_some() {
            DeviceClass::Handheld
        } else {
            DeviceClass::Desktop
        };
        #[cfg(not(target_os = "android"))]
        let class = DeviceClass::Desktop;
        let touch = class == DeviceClass::Handheld || std::env::args().any(|arg| arg == "--touch");
        DeviceProfile::detect(class, None, touch)
    }

    /// Starts the event loop before building the app, so engine startup follows platform startup.
    pub fn run_with<A, E>(
        &self,
        title: &str,
        size: (u32, u32),
        build: impl FnOnce() -> Result<A, E>,
    ) -> Result<(), E>
    where
        A: crate::PlatformApp,
        E: From<PlatformError>,
    {
        #[cfg(target_os = "android")]
        let event_loop = {
            use winit::platform::android::EventLoopBuilderExtAndroid;
            match self.android.as_ref() {
                Some(android) => winit::event_loop::EventLoop::builder()
                    .with_android_app(android.clone())
                    .build()
                    .map_err(PlatformError::from)
                    .map_err(E::from)?,
                None => winit::event_loop::EventLoop::new()
                    .map_err(PlatformError::from)
                    .map_err(E::from)?,
            }
        };
        #[cfg(not(target_os = "android"))]
        let event_loop = winit::event_loop::EventLoop::new()
            .map_err(PlatformError::from)
            .map_err(E::from)?;
        let app = build()?;
        crate::window::run_on(event_loop, title, size, app).map_err(E::from)
    }
}

const DEVELOPMENT_SEARCH_DEPTH: usize = 2;

fn find_development_pack(root: &Path, name: &str) -> std::io::Result<Option<PathBuf>> {
    let mut roots = vec![root.to_owned()];
    for depth in 0..=DEVELOPMENT_SEARCH_DEPTH {
        roots.sort();
        let mut matches = roots
            .iter()
            .map(|candidate| game_content_pack_path(candidate, name))
            .filter(|candidate| candidate.is_file());
        if let Some(found) = matches.next() {
            if matches.next().is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("more than one development content pack is named {name}"),
                ));
            }
            return Ok(Some(found));
        }
        if depth == DEVELOPMENT_SEARCH_DEPTH {
            break;
        }
        let mut children = Vec::new();
        for candidate in &roots {
            for entry in std::fs::read_dir(candidate)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    children.push(entry.path());
                }
            }
        }
        roots = children;
    }
    Ok(None)
}

#[cfg(test)]
mod content_tests {
    use super::*;

    #[test]
    fn development_lookup_uses_the_canonical_game_pack_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "engine-platform-content-search-{}",
            std::process::id()
        ));
        let game_root = root.join("workspace").join("sample");
        let pack = game_content_pack_path(&game_root, "sample");
        let parent = pack.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "pack path has no parent")
        })?;
        std::fs::create_dir_all(parent)?;
        std::fs::write(&pack, b"pack")?;

        let found = find_development_pack(&root, "sample")?;
        assert_eq!(found.as_deref(), Some(pack.as_path()));
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
}
