//! The `Audio` manager: starts the Kira backend and owns the buses, bank, voices and listener.
//!
//! Invariant: the sub-track capacity is fixed before any bank is known, and the global voice cap
//! is at least one (`with_voice_limit` rejects zero).

use std::path::Path;

use glam::{Quat, Vec3};
use kira::{
    AudioManager, AudioManagerSettings, Tween,
    backend::{Backend, DefaultBackend},
    listener::ListenerHandle,
    track::TrackBuilder,
};

use super::buses::Buses;
use super::playback::Voice;
use super::spatial::SpatialPool;
use crate::bank::{SoundBank, SoundId};
use crate::error::AudioError;
use crate::player::{AudioPlayer, Emit};
use crate::volume::Volume;

/// The maximum simultaneous voices when the caller does not request another cap.
pub const DEFAULT_GLOBAL_VOICE_LIMIT: usize = 32;
// Kira's track capacity is fixed when the manager starts, before the sound bank reveals how many
// bus and attenuation profiles need global-sized pools.
const MIXER_SUB_TRACK_CAPACITY: usize = 4_096;

/// Audio output with a Kira backend and a bounded voice list.
pub struct Audio<B: Backend = DefaultBackend> {
    pub(super) manager: AudioManager<B>,
    pub(super) listener: ListenerHandle,
    pub(super) buses: Buses,
    pub(super) spatial_pools: Vec<SpatialPool>,
    pub(super) bank: SoundBank,
    pub(super) voices: Vec<Voice>,
    pub(super) global_voice_limit: usize,
    pub(super) sequence: u64,
    pub(super) pitch_rng: u64,
    pub(super) volumes: [Volume; 7],
    pub(super) culled_plays: u64,
}

impl<B: Backend> Audio<B> {
    /// Creates an audio manager using the requested Kira backend settings.
    pub fn new(backend: B::Settings) -> Result<Self, AudioError> {
        let settings = AudioManagerSettings {
            backend_settings: backend,
            capacities: kira::Capacities {
                sub_track_capacity: MIXER_SUB_TRACK_CAPACITY,
                ..Default::default()
            },
            main_track_builder: Default::default(),
            internal_buffer_size: 128,
        };
        let mut manager = AudioManager::<B>::new(settings).map_err(|_| AudioError::BackendStart)?;
        let listener = manager
            .add_listener([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0])
            .map_err(|_| AudioError::MixerCapacity)?;
        let music = manager
            .add_sub_track(TrackBuilder::new())
            .map_err(|_| AudioError::MixerCapacity)?;
        let mut sfx = manager
            .add_sub_track(TrackBuilder::new())
            .map_err(|_| AudioError::MixerCapacity)?;
        let voice = manager
            .add_sub_track(TrackBuilder::new())
            .map_err(|_| AudioError::MixerCapacity)?;
        let master_spatial = manager
            .add_sub_track(TrackBuilder::new())
            .map_err(|_| AudioError::MixerCapacity)?;
        let weapons = sfx
            .add_sub_track(TrackBuilder::new())
            .map_err(|_| AudioError::MixerCapacity)?;
        let foley = sfx
            .add_sub_track(TrackBuilder::new())
            .map_err(|_| AudioError::MixerCapacity)?;
        let ui = sfx
            .add_sub_track(TrackBuilder::new())
            .map_err(|_| AudioError::MixerCapacity)?;
        Ok(Self {
            manager,
            listener,
            buses: Buses {
                master_spatial,
                music,
                sfx,
                weapons,
                foley,
                ui,
                voice,
            },
            spatial_pools: Vec::new(),
            bank: SoundBank::default(),
            voices: Vec::with_capacity(DEFAULT_GLOBAL_VOICE_LIMIT),
            global_voice_limit: DEFAULT_GLOBAL_VOICE_LIMIT,
            sequence: 0,
            pitch_rng: 0x6a09_e667_f3bc_c909,
            volumes: [Volume(1.0); 7],
            culled_plays: 0,
        })
    }

    /// Creates an audio manager with an explicit global voice cap.
    pub fn with_voice_limit(
        backend: B::Settings,
        global_voice_limit: usize,
    ) -> Result<Self, AudioError> {
        if global_voice_limit == 0 {
            return Err(AudioError::ZeroVoiceLimit);
        }
        let mut audio = Self::new(backend)?;
        audio.global_voice_limit = global_voice_limit;
        audio.voices = Vec::with_capacity(global_voice_limit);
        Ok(audio)
    }

    /// Updates the world-space listener pose.
    pub fn set_listener(&mut self, position: Vec3, rotation: Quat) {
        self.listener
            .set_position([position.x, position.y, position.z], Tween::default());
        self.listener.set_orientation(
            [rotation.x, rotation.y, rotation.z, rotation.w],
            Tween::default(),
        );
    }

    /// Number of low-priority plays dropped instead of displacing louder voices.
    #[must_use]
    pub const fn culled_plays(&self) -> u64 {
        self.culled_plays
    }

    /// Number of currently tracked active voices.
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }

    /// The loaded definition's path, for diagnostics.
    #[must_use]
    pub fn sound_path(&self, id: SoundId) -> Option<&Path> {
        self.bank
            .sounds
            .get(id.0)
            .map(|sound| sound.definition.file.as_path())
    }
}

impl Audio<DefaultBackend> {
    /// Creates an audio manager on the real device backend.
    pub fn new_default() -> Result<Self, AudioError> {
        Self::new(kira::backend::cpal::CpalBackendSettings::default())
    }
}

/// Builds a device-free audio player backed by Kira's mock renderer.
pub fn new_mock() -> Result<impl AudioPlayer, AudioError> {
    Audio::<kira::backend::mock::MockBackend>::new(
        kira::backend::mock::MockBackendSettings::default(),
    )
}

impl<B: Backend> AudioPlayer for Audio<B> {
    fn set_bank(&mut self, bank: SoundBank) -> Result<(), AudioError> {
        Audio::set_bank(self, bank)
    }

    fn play(&mut self, id: SoundId, emit: Emit) -> Result<(), AudioError> {
        Audio::play(self, id, emit)
    }

    fn active_voices(&self) -> usize {
        Audio::active_voices(self)
    }
}
