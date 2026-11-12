//! Decoded sound bank: loads and validates definitions once, then hands out typed `SoundId`s.
//!
//! Invariant: every sound in a loaded bank has a valid definition and decoded data, and a
//! `SoundId` is only minted for an index that exists in the bank.

use std::io::Cursor;

use kira::sound::static_sound::StaticSoundData;

use crate::attenuation::Attenuation;
use crate::definition::SoundDef;
use crate::error::AudioError;

/// A typed sound-bank index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundId(pub(crate) usize);

impl SoundId {
    /// Index used to build this id from a bank entry.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

pub(crate) struct Sound {
    pub(crate) definition: SoundDef,
    pub(crate) data: StaticSoundData,
}

/// Decoded sounds, loaded once from game-provided definitions.
#[derive(Default)]
pub struct SoundBank {
    pub(crate) sounds: Vec<Sound>,
}

impl SoundBank {
    /// Loads and decodes all definitions before the bank is used for playback.
    pub fn load(definitions: Vec<SoundDef>) -> Result<Self, AudioError> {
        let mut sounds = Vec::with_capacity(definitions.len());
        for definition in definitions {
            validate_definition(&definition)?;
            let path = definition.file.clone();
            let data = StaticSoundData::from_file(&path)
                .map_err(|source| AudioError::SoundFile { path, source })?;
            sounds.push(Sound { definition, data });
        }
        Ok(Self { sounds })
    }

    /// Decodes game-embedded sound files, used by Android and other packaged builds.
    pub fn load_embedded(
        definitions: Vec<SoundDef>,
        contents: Vec<Vec<u8>>,
    ) -> Result<Self, AudioError> {
        if definitions.len() != contents.len() {
            return Err(AudioError::EmbeddedSoundCount {
                expected: definitions.len(),
                actual: contents.len(),
            });
        }
        let mut sounds = Vec::with_capacity(definitions.len());
        for (index, definition) in definitions.into_iter().enumerate() {
            validate_definition(&definition)?;
            let Some(bytes) = contents.get(index) else {
                return Err(AudioError::EmbeddedSoundMissing(definition.file));
            };
            let path = definition.file.clone();
            let data = StaticSoundData::from_cursor(Cursor::new(bytes))
                .map_err(|source| AudioError::SoundFile { path, source })?;
            sounds.push(Sound { definition, data });
        }
        Ok(Self { sounds })
    }

    /// Number of decoded sounds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sounds.len()
    }

    /// Whether no sounds were loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }

    /// The id for a successfully loaded entry.
    #[must_use]
    pub fn id(&self, index: usize) -> Option<SoundId> {
        (index < self.sounds.len()).then_some(SoundId(index))
    }
}

fn validate_definition(definition: &SoundDef) -> Result<(), AudioError> {
    if definition.max_voices == 0
        || !definition.pitch_jitter.is_finite()
        || !(0.0..=1.0).contains(&definition.pitch_jitter)
    {
        return Err(AudioError::InvalidSoundDef(definition.file.clone()));
    }
    if let Some(range) = definition.spatial {
        Attenuation::new(range.min_distance(), range.max_distance())?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use crate::volume::Volume;
    use std::path::PathBuf;

    #[test]
    fn missing_sound_file_reports_its_path() {
        let path = PathBuf::from("missing-audio-test.wav");
        let definition = SoundDef {
            file: path.clone(),
            bus: Bus::Weapons,
            volume: Volume(1.0),
            max_voices: 1,
            priority: 0,
            spatial: None,
            pitch_jitter: 0.0,
        };
        assert!(
            matches!(SoundBank::load(vec![definition]), Err(AudioError::SoundFile { path: error_path, .. }) if error_path == path)
        );
    }
}
