//! Player display names, validated once at the edge so every name inside a room is printable
//! and short enough for any HUD.

use serde::{Deserialize, Serialize};

use crate::error::SessionError;

/// Longest name, in characters (not bytes).
pub const MAX_NAME_CHARS: usize = 16;

/// A player's display name: 1 to [`MAX_NAME_CHARS`] characters after trimming, no control
/// characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlayerName(String);

impl PlayerName {
    /// Validates and trims a name.
    ///
    /// # Errors
    /// [`SessionError::InvalidName`] when the trimmed name is empty, too long, or holds a
    /// control character.
    pub fn new(text: &str) -> Result<Self, SessionError> {
        let trimmed = text.trim();
        let chars = trimmed.chars().count();
        if chars == 0 || chars > MAX_NAME_CHARS || trimmed.chars().any(char::is_control) {
            return Err(SessionError::InvalidName);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PlayerName {
    type Error = SessionError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(&text)
    }
}

impl From<PlayerName> for String {
    fn from(name: PlayerName) -> Self {
        name.0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_bounded_in_characters() {
        assert_eq!(PlayerName::new("  Ada ").unwrap().as_str(), "Ada");
        assert_eq!(
            PlayerName::new(&"é".repeat(16))
                .unwrap()
                .as_str()
                .chars()
                .count(),
            16
        );
        assert_eq!(
            PlayerName::new(&"a".repeat(17)),
            Err(SessionError::InvalidName)
        );
        assert_eq!(PlayerName::new("   "), Err(SessionError::InvalidName));
        assert_eq!(PlayerName::new("a\nb"), Err(SessionError::InvalidName));
    }
}
