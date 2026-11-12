//! Spatial track pools: one pool per (bus, attenuation) profile, built when a bank is installed.
//!
//! Invariant: Kira fixes a spatial track's distances at creation, so pools are created once in
//! `set_bank`; a play only borrows a free slot. A pool never holds more tracks than its sounds'
//! voice limits allow, capped by the global voice limit.

use kira::{
    Tween,
    backend::Backend,
    track::{SpatialTrackBuilder, SpatialTrackHandle},
};

use super::output::Audio;
use crate::attenuation::Attenuation;
use crate::bank::SoundBank;
use crate::bus::Bus;
use crate::error::AudioError;

/// Kira 0.12 fixes attenuation distances when a spatial track is created, so tracks are pooled
/// once per bus and distance profile when a bank is installed.
pub(super) struct SpatialPool {
    pub(super) bus: Bus,
    pub(super) attenuation: Attenuation,
    pub(super) tracks: Vec<SpatialTrackHandle>,
}

impl<B: Backend> Audio<B> {
    /// Installs a fully decoded sound bank.
    pub fn set_bank(&mut self, bank: SoundBank) -> Result<(), AudioError> {
        for voice in &mut self.voices {
            voice.handle.stop(Tween::default());
        }
        self.voices.clear();
        self.spatial_pools.clear();
        let mut pools = Vec::<SpatialPool>::new();
        let listener_id = self.listener.id();
        for sound in &bank.sounds {
            let Some(attenuation) = sound.definition.spatial else {
                continue;
            };
            let bus = sound.definition.bus;
            if pools
                .iter()
                .any(|pool| pool.bus == bus && pool.attenuation == attenuation)
            {
                continue;
            }
            // A profile never holds more voices than its sounds' own limits allow, so idle
            // spatial tracks (which Kira still processes every chunk) are not created.
            let capacity = bank
                .sounds
                .iter()
                .filter(|other| {
                    other.definition.bus == bus && other.definition.spatial == Some(attenuation)
                })
                .map(|other| other.definition.max_voices)
                .fold(0_usize, usize::saturating_add)
                .min(self.global_voice_limit);
            let mut tracks = Vec::with_capacity(capacity);
            for _ in 0..capacity {
                let track = self
                    .spatial_parent_mut(bus)
                    .add_spatial_sub_track(
                        listener_id,
                        [0.0, 0.0, 0.0],
                        SpatialTrackBuilder::new()
                            .distances((attenuation.min_distance(), attenuation.max_distance())),
                    )
                    .map_err(|_| AudioError::MixerCapacity)?;
                tracks.push(track);
            }
            pools.push(SpatialPool {
                bus,
                attenuation,
                tracks,
            });
        }
        self.spatial_pools = pools;
        self.bank = bank;
        Ok(())
    }

    pub(super) fn free_spatial_slot(&self, pool_index: usize) -> Result<usize, AudioError> {
        let pool = self
            .spatial_pools
            .get(pool_index)
            .ok_or(AudioError::MixerCapacity)?;
        pool.tracks
            .iter()
            .enumerate()
            .find(|(slot_index, _)| {
                !self
                    .voices
                    .iter()
                    .any(|voice| voice.spatial_slot == Some((pool_index, *slot_index)))
            })
            .map(|(slot_index, _)| slot_index)
            .ok_or(AudioError::MixerCapacity)
    }
}
