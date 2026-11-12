//! Validated logical names used to address entries in a game's content pack.
use std::borrow::Cow;
use std::fmt::{Display, Formatter};

use thiserror::Error;

/// A relative, slash-separated key inside a content pack.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetPath(String);

/// A validated game content-pack name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentName(Cow<'static, str>);

/// A logical path or pack name did not meet its portable naming rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid content name")]
pub struct InvalidContentName;

impl AssetPath {
    /// Validates a non-empty relative path with forward slashes and no parent segments.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidContentName> {
        let value = value.into();
        let components = value.split('/').collect::<Vec<_>>();
        if value.is_empty()
            || value.starts_with('/')
            || value.contains('\\')
            || value.contains(':')
            || components
                .iter()
                .any(|part| part.is_empty() || *part == "." || *part == "..")
        {
            return Err(InvalidContentName);
        }
        Ok(Self(value))
    }

    /// Returns the canonical slash-separated path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl ContentName {
    /// Validates a portable lower-case pack name.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidContentName> {
        let value = value.into();
        if value.is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || !value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        {
            return Err(InvalidContentName);
        }
        Ok(Self(Cow::Owned(value)))
    }

    /// Creates a pack name embedded as a game entry constant.
    #[must_use]
    pub const fn from_static(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }

    /// Returns the pack name without a file extension.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl Display for AssetPath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0.as_ref())
    }
}

impl Display for ContentName {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0.as_ref())
    }
}
