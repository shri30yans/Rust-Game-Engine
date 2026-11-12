//! One-shot playback admission: voice limits, culling, pitch jitter and routing to a voice.
//!
//! Invariant: finished voices are released before admission, a per-sound limit is enforced before
//! the global cap, and the voice list never exceeds the global cap.

use kira::{Decibels, Tween, backend::Backend, sound::static_sound::StaticSoundHandle};

use super::output::Audio;
use crate::bank::SoundId;
use crate::bus::Bus;
use crate::error::AudioError;
use crate::player::Emit;

pub(super) struct Voice {
    pub(super) id: SoundId,
    pub(super) priority: i32,
    pub(super) started: u64,
    pub(super) handle: StaticSoundHandle,
    pub(super) spatial_slot: Option<(usize, usize)>,
}

impl<B: Backend> Audio<B> {
    /// Plays a loaded one-shot, applying its sound concurrency limit and the global cap.
    pub fn play(&mut self, id: SoundId, emit: Emit) -> Result<(), AudioError> {
        let Some(sound) = self.bank.sounds.get(id.0) else {
            return Err(AudioError::UnknownSound(id.0));
        };
        let max_voices = sound.definition.max_voices;
        let jitter = sound.definition.pitch_jitter;
        let volume = sound.definition.volume;
        let sound_bus = sound.definition.bus;
        let attenuation = sound.definition.spatial;
        let priority = sound.definition.priority;
        self.release_finished_voices();
        let same_sound = self.voices.iter().filter(|voice| voice.id == id).count();
        let per_sound_full = same_sound >= max_voices;
        let globally_full = self.voices.len() >= self.global_voice_limit;
        if per_sound_full || globally_full {
            let victim = self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| !per_sound_full || voice.id == id)
                .min_by_key(|(_, voice)| (voice.priority, voice.started))
                .map(|(index, voice)| (index, voice.priority));
            let Some((index, victim_priority)) = victim else {
                return Err(AudioError::MixerCapacity);
            };
            if priority < victim_priority {
                self.culled_plays = self.culled_plays.wrapping_add(1);
                return Ok(());
            }
            if let Some(voice) = self.voices.get_mut(index) {
                voice.handle.stop(Tween::default());
            }
            self.voices.swap_remove(index);
        }
        // Kira owns the rendering path; this wrapper handles stable routing and admission.
        let random = self.next_pitch_unit();
        let pitch = 1.0 + (random * 2.0 - 1.0) * jitter;
        let sound_data = self
            .bank
            .sounds
            .get(id.0)
            .ok_or(AudioError::UnknownSound(id.0))?
            .data
            .clone();
        let data = sound_data
            .volume(Decibels::from(volume.decibels()))
            .playback_rate(f64::from(pitch));
        let bus = if matches!(emit, Emit::Ui) {
            Bus::Ui
        } else {
            sound_bus
        };
        let (handle, spatial_slot) = match (emit, attenuation) {
            (Emit::At(position), Some(range)) => {
                let pool_index = self
                    .spatial_pools
                    .iter()
                    .position(|pool| pool.bus == bus && pool.attenuation == range)
                    .ok_or(AudioError::MixerCapacity)?;
                let slot_index = self.free_spatial_slot(pool_index)?;
                let pool = self
                    .spatial_pools
                    .get_mut(pool_index)
                    .ok_or(AudioError::MixerCapacity)?;
                let track = pool
                    .tracks
                    .get_mut(slot_index)
                    .ok_or(AudioError::MixerCapacity)?;
                track.set_position([position.x, position.y, position.z], Tween::default());
                (
                    track.play(data).map_err(|_| AudioError::MixerCapacity)?,
                    Some((pool_index, slot_index)),
                )
            }
            _ => (self.play_on_bus(bus, data)?, None),
        };
        self.sequence = self.sequence.wrapping_add(1);
        self.voices.push(Voice {
            id,
            priority,
            started: self.sequence,
            handle,
            spatial_slot,
        });
        Ok(())
    }

    fn next_pitch_unit(&mut self) -> f32 {
        self.pitch_rng ^= self.pitch_rng << 13;
        self.pitch_rng ^= self.pitch_rng >> 7;
        self.pitch_rng ^= self.pitch_rng << 17;
        let fraction_bits = u32::try_from(self.pitch_rng >> 41).unwrap_or(0);
        f32::from_bits(0x3f80_0000 | fraction_bits) - 1.0
    }

    fn release_finished_voices(&mut self) {
        let mut index = 0;
        while index < self.voices.len() {
            let finished = self
                .voices
                .get(index)
                .is_some_and(|voice| voice.handle.state() != kira::sound::PlaybackState::Playing);
            if finished {
                self.voices.swap_remove(index);
            } else {
                index += 1;
            }
        }
    }
}
