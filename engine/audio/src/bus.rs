//! The mixer buses a one-shot is routed through.
//!
//! Invariant: the enum is closed, so every routing match is exhaustive; each bus is mapped to a
//! Kira track in `audio::buses`.

/// The mixer bus a sound is routed through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bus {
    /// All output.
    Master,
    /// Background music.
    Music,
    /// Sound effects parent bus.
    Sfx,
    /// Weapon sounds, under [`Bus::Sfx`].
    Weapons,
    /// Movement and other foley, under [`Bus::Sfx`].
    Foley,
    /// Interface sounds, under [`Bus::Sfx`].
    Ui,
    /// Voice output.
    Voice,
}
