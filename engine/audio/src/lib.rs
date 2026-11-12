//! Game-agnostic sound loading, mixer buses and bounded one-shot playback.
//!
//! Kira stays behind this crate. Sound ids and buses are typed, sound files are decoded during
//! loading, and the active voice list is reserved before playback begins.
#![forbid(unsafe_code)]

mod attenuation;
mod audio;
mod bank;
mod bus;
mod definition;
mod error;
mod player;
mod volume;

pub use attenuation::Attenuation;
pub use audio::{Audio, DEFAULT_GLOBAL_VOICE_LIMIT, new_mock};
pub use bank::{SoundBank, SoundId};
pub use bus::Bus;
pub use definition::SoundDef;
pub use error::AudioError;
pub use player::{AudioPlayer, Emit};
pub use volume::Volume;
