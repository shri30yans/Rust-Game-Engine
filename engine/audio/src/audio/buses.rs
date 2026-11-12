//! Bus routing: maps each `Bus` to its Kira track and applies per-bus volume.
//!
//! Invariant: `Bus::Master` is the main track and every other bus is a sub-track, so a bus always
//! resolves to exactly one track.

use kira::{
    Decibels, Tween,
    backend::Backend,
    sound::static_sound::{StaticSoundData, StaticSoundHandle},
    track::{MainTrackHandle, TrackHandle},
};

use super::output::Audio;
use crate::bus::Bus;
use crate::error::AudioError;
use crate::volume::Volume;

pub(super) struct Buses {
    pub(super) master_spatial: TrackHandle,
    pub(super) music: TrackHandle,
    pub(super) sfx: TrackHandle,
    pub(super) weapons: TrackHandle,
    pub(super) foley: TrackHandle,
    pub(super) ui: TrackHandle,
    pub(super) voice: TrackHandle,
}

enum TrackRef<'a> {
    Main(&'a mut MainTrackHandle),
    Sub(&'a mut TrackHandle),
}

impl<B: Backend> Audio<B> {
    /// Sets linear gain for a bus, converting it to decibels only when the value changes.
    pub fn set_volume(&mut self, bus: Bus, volume: Volume) {
        let index = bus_index(bus);
        if self
            .volumes
            .get(index)
            .is_some_and(|current| *current == volume)
        {
            return;
        }
        if let Some(slot) = self.volumes.get_mut(index) {
            *slot = volume;
        }
        match self.track_mut(bus) {
            TrackRef::Main(track) => {
                track.set_volume(Decibels::from(volume.decibels()), Tween::default());
            }
            TrackRef::Sub(track) => {
                track.set_volume(Decibels::from(volume.decibels()), Tween::default());
            }
        }
    }

    /// Gets the current bus volume.
    #[must_use]
    pub fn volume(&self, bus: Bus) -> Volume {
        self.volumes
            .get(bus_index(bus))
            .copied()
            .unwrap_or(Volume(1.0))
    }

    fn track_mut(&mut self, bus: Bus) -> TrackRef<'_> {
        match bus {
            Bus::Master => TrackRef::Main(self.manager.main_track()),
            Bus::Music => TrackRef::Sub(&mut self.buses.music),
            Bus::Sfx => TrackRef::Sub(&mut self.buses.sfx),
            Bus::Weapons => TrackRef::Sub(&mut self.buses.weapons),
            Bus::Foley => TrackRef::Sub(&mut self.buses.foley),
            Bus::Ui => TrackRef::Sub(&mut self.buses.ui),
            Bus::Voice => TrackRef::Sub(&mut self.buses.voice),
        }
    }

    pub(super) fn spatial_parent_mut(&mut self, bus: Bus) -> &mut TrackHandle {
        match bus {
            Bus::Master => &mut self.buses.master_spatial,
            Bus::Music => &mut self.buses.music,
            Bus::Sfx => &mut self.buses.sfx,
            Bus::Weapons => &mut self.buses.weapons,
            Bus::Foley => &mut self.buses.foley,
            Bus::Ui => &mut self.buses.ui,
            Bus::Voice => &mut self.buses.voice,
        }
    }

    pub(super) fn play_on_bus(
        &mut self,
        bus: Bus,
        data: StaticSoundData,
    ) -> Result<StaticSoundHandle, AudioError> {
        match self.track_mut(bus) {
            TrackRef::Main(track) => track.play(data).map_err(|_| AudioError::MixerCapacity),
            TrackRef::Sub(track) => track.play(data).map_err(|_| AudioError::MixerCapacity),
        }
    }
}

const fn bus_index(bus: Bus) -> usize {
    match bus {
        Bus::Master => 0,
        Bus::Music => 1,
        Bus::Sfx => 2,
        Bus::Weapons => 3,
        Bus::Foley => 4,
        Bus::Ui => 5,
        Bus::Voice => 6,
    }
}
