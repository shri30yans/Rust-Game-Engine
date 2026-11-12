//! Validation contract for game-owned tuning values loaded from logical content paths.

/// A game's tuning type validates its own numeric and cross-field rules after parsing.
pub trait Validate {
    /// Returns a message when the value is outside the game's valid range.
    fn validate(&self) -> Result<(), String>;
}
