//! IMAGINE's sound data validation and presentation of existing simulation feedback.
//!
//! The presenter samples per-tick feedback once per rendered frame. Footstep distance, last hit
//! marker and last kill flash are presentation state only and never enter the simulation hash.

use std::{collections::BTreeMap, path::PathBuf};

use app::{AppError, Attenuation, AudioPlayer, Bus, Emit, SoundBank, SoundDef, SoundId, Volume};
use engine_assets::{AssetPath, ContentPack};
use engine_world::World;
use glam::Vec3;
use serde::Deserialize;
use thiserror::Error;

use crate::{
    feedback::HudEvents,
    rules::{Actors, PLAYER_SLOT},
    sim::is_alive,
};
use kit_fps::{Locomotion, Weapon};

const SOUND_CONFIG_PATH: &str = "data/sounds.ron";
const SOUND_DIRECTORY: &str = "content/audio/placeholder";
const SOUND_HASH_PRIME: u64 = 0x0000_0100_0000_01b3;
const SHOT_NEAR_DISTANCE_M: f32 = 15.0;
const FOOTSTEP_SPEED_MPS: f32 = 0.15;

/// One of the authored sound names in `sounds.ron`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SoundName {
    RifleNear,
    RifleFar,
    #[serde(rename = "footstep_1")]
    Footstep1,
    #[serde(rename = "footstep_2")]
    Footstep2,
    #[serde(rename = "footstep_3")]
    Footstep3,
    HitTick,
    KillConfirm,
    DryFire,
}

impl SoundName {
    const COUNT: usize = 8;
    const ALL: [Self; Self::COUNT] = [
        Self::RifleNear,
        Self::RifleFar,
        Self::Footstep1,
        Self::Footstep2,
        Self::Footstep3,
        Self::HitTick,
        Self::KillConfirm,
        Self::DryFire,
    ];
}

struct SoundIds {
    rifle_near: SoundId,
    rifle_far: SoundId,
    footstep_1: SoundId,
    footstep_2: SoundId,
    footstep_3: SoundId,
    hit_tick: SoundId,
    kill_confirm: SoundId,
    dry_fire: SoundId,
}

impl SoundIds {
    fn id(&self, name: SoundName) -> SoundId {
        match name {
            SoundName::RifleNear => self.rifle_near,
            SoundName::RifleFar => self.rifle_far,
            SoundName::Footstep1 => self.footstep_1,
            SoundName::Footstep2 => self.footstep_2,
            SoundName::Footstep3 => self.footstep_3,
            SoundName::HitTick => self.hit_tick,
            SoundName::KillConfirm => self.kill_confirm,
            SoundName::DryFire => self.dry_fire,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum SoundBus {
    Weapons,
    Foley,
    Ui,
}

impl From<SoundBus> for Bus {
    fn from(bus: SoundBus) -> Self {
        match bus {
            SoundBus::Weapons => Self::Weapons,
            SoundBus::Foley => Self::Foley,
            SoundBus::Ui => Self::Ui,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpatialData {
    min_distance: f32,
    max_distance: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SoundEntry {
    id: SoundName,
    file: String,
    bus: SoundBus,
    volume: f32,
    pitch_jitter: f32,
    max_voices: usize,
    priority: i32,
    spatial: Option<SpatialData>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SoundConfig {
    footstep_stride_m: f32,
    sounds: Vec<SoundEntry>,
}

/// Configuration errors identify the affected source file and field.
#[derive(Debug, Error)]
pub enum SoundConfigError {
    /// The RON file could not be parsed.
    #[error("{file}: {source}")]
    Parse {
        /// The config file.
        file: PathBuf,
        /// The parse error.
        #[source]
        source: ron::error::SpannedError,
    },
    /// A named sound file is absent.
    #[error("{file}: missing file in field `{field}`")]
    MissingFile {
        /// The missing asset path.
        file: PathBuf,
        /// The data field that names the file.
        field: &'static str,
    },
    /// A config value does not satisfy the sound data invariants.
    #[error("{file}: invalid field `{field}`")]
    InvalidField {
        /// The config or asset file.
        file: PathBuf,
        /// The invalid data field.
        field: &'static str,
    },
    /// A named sound could not be decoded or installed.
    #[error(transparent)]
    Audio(#[from] app::AudioError),
}

/// Counts presentation play requests for scripted determinism checks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AudioPlayCounts {
    /// Shot one-shots requested.
    pub shots: u32,
    /// Footstep one-shots requested.
    pub footsteps: u32,
    /// Local hit marker one-shots requested.
    pub hits: u32,
    /// Local kill confirmation one-shots requested.
    pub kills: u32,
}

#[derive(Debug, Clone, Copy, Default)]
struct FootstepState {
    last_position: Option<Vec3>,
    distance_m: f32,
    variant: usize,
}

/// Per-presenter sound state and already decoded assets.
pub struct ImagineAudio {
    bank: Option<SoundBank>,
    ids: SoundIds,
    stride_m: f32,
    footsteps: Vec<FootstepState>,
    last_shots: Vec<u32>,
    last_hits: u32,
    last_kills: u32,
    counts: AudioPlayCounts,
}

impl ImagineAudio {
    /// Computes the pack revision for the sound definition and every named sound entry.
    pub fn content_hash(content: &ContentPack) -> Result<u64, SoundConfigError> {
        let config_path =
            AssetPath::new(SOUND_CONFIG_PATH).map_err(|_| SoundConfigError::InvalidField {
                file: PathBuf::from(SOUND_CONFIG_PATH),
                field: "path",
            })?;
        let source =
            content
                .load_text(&config_path)
                .map_err(|_| SoundConfigError::MissingFile {
                    file: PathBuf::from(SOUND_CONFIG_PATH),
                    field: "source",
                })?;
        let config: SoundConfig =
            ron::from_str(source).map_err(|source| SoundConfigError::Parse {
                file: PathBuf::from(SOUND_CONFIG_PATH),
                source,
            })?;
        let (named, _) = sound_definitions(config)?;
        named.iter().try_fold(
            content
                .hash(&config_path)
                .map_err(|_| SoundConfigError::MissingFile {
                    file: PathBuf::from(SOUND_CONFIG_PATH),
                    field: "source",
                })?,
            |hash, (_, definition)| {
                let path = AssetPath::new(definition.file.to_string_lossy().replace('\\', "/"))
                    .map_err(|_| SoundConfigError::InvalidField {
                        file: definition.file.clone(),
                        field: "file",
                    })?;
                let entry_hash =
                    content
                        .hash(&path)
                        .map_err(|_| SoundConfigError::MissingFile {
                            file: definition.file.clone(),
                            field: "file",
                        })?;
                Ok::<u64, SoundConfigError>(hash.wrapping_mul(SOUND_HASH_PRIME) ^ entry_hash)
            },
        )
    }

    /// Loads and decodes the authored sound bank for this presenter.
    pub fn new(content: &ContentPack) -> Result<Self, SoundConfigError> {
        let (bank, ids, stride_m) = load_bank(content)?;
        Ok(Self {
            bank: Some(bank),
            ids,
            stride_m,
            footsteps: Vec::new(),
            last_shots: Vec::new(),
            last_hits: 0,
            last_kills: 0,
            counts: AudioPlayCounts::default(),
        })
    }

    /// Clears event cursors and movement history when a new world replaces the old one.
    pub fn on_travel(&mut self) {
        self.footsteps.clear();
        self.last_shots.clear();
        self.last_hits = 0;
        self.last_kills = 0;
    }

    /// Plays this frame's shot, grounded movement, hit and kill sounds.
    pub fn play_frame<A: AudioPlayer>(
        &mut self,
        world: &World,
        audio: &mut A,
    ) -> Result<(), AppError> {
        if let Some(bank) = self.bank.take() {
            audio.set_bank(bank)?;
        }
        let actor_count = world.resource::<Actors>()?.slots().len();
        if self.footsteps.len() != actor_count {
            self.footsteps = vec![FootstepState::default(); actor_count];
            self.last_shots = vec![0; actor_count];
        }
        self.play_events(world, audio)
    }

    /// Counts requested playback events, used by deterministic match tests.
    #[must_use]
    pub const fn counts(&self) -> AudioPlayCounts {
        self.counts
    }

    fn play_events<A: AudioPlayer>(
        &mut self,
        world: &World,
        audio: &mut A,
    ) -> Result<(), AppError> {
        let player = world.resource::<Actors>()?.get(PLAYER_SLOT).ok_or(
            engine_world::WorldError::Invariant("the player actor is missing"),
        )?;
        let listener = world.get::<Locomotion>(player.id())?.eye();
        let events = world.resource::<HudEvents>()?;
        let actors = world.resource::<Actors>()?;
        for (slot, count) in events.shots_fired().iter().copied().enumerate() {
            let previous =
                self.last_shots
                    .get_mut(slot)
                    .ok_or(engine_world::WorldError::Invariant(
                        "shot counters do not match actor slots",
                    ))?;
            let advanced = count.wrapping_sub(*previous);
            *previous = count;
            if advanced == 0 {
                continue;
            }
            let actor = actors
                .slots()
                .get(slot)
                .ok_or(engine_world::WorldError::Invariant(
                    "shot counters do not match actor slots",
                ))?;
            let position = world.get::<Locomotion>(actor.id())?.eye();
            let distance = (position - listener).length();
            let id = if distance <= SHOT_NEAR_DISTANCE_M {
                self.sound_id(SoundName::RifleNear)
            } else {
                self.sound_id(SoundName::RifleFar)
            };
            for _ in 0..advanced {
                audio.play(id, Emit::At(position))?;
                self.counts.shots = self.counts.shots.saturating_add(1);
            }
        }
        let player_locomotion = world.get::<Locomotion>(player.id())?;
        if player_locomotion.command().fire()
            && world.get::<Weapon>(player.id())?.state().ammo() == 0
        {
            audio.play(self.sound_id(SoundName::DryFire), Emit::At(listener))?;
        }
        self.play_footsteps(world, audio)?;
        let hits = events.player_hits().wrapping_sub(self.last_hits);
        self.last_hits = events.player_hits();
        for _ in 0..hits {
            audio.play(self.sound_id(SoundName::HitTick), Emit::Ui)?;
            self.counts.hits = self.counts.hits.saturating_add(1);
        }
        let kills = events.player_kills().wrapping_sub(self.last_kills);
        self.last_kills = events.player_kills();
        for _ in 0..kills {
            audio.play(self.sound_id(SoundName::KillConfirm), Emit::Ui)?;
            self.counts.kills = self.counts.kills.saturating_add(1);
        }
        Ok(())
    }

    fn play_footsteps<A: AudioPlayer>(
        &mut self,
        world: &World,
        audio: &mut A,
    ) -> Result<(), AppError> {
        let footstep_ids = [
            self.sound_id(SoundName::Footstep1),
            self.sound_id(SoundName::Footstep2),
            self.sound_id(SoundName::Footstep3),
        ];
        let slots = world.resource::<Actors>()?.slots();
        for (slot_index, actor) in slots.iter().enumerate() {
            if !is_alive(world, slot_index)? {
                continue;
            }
            let locomotion = *world.get::<Locomotion>(actor.id())?;
            let state =
                self.footsteps
                    .get_mut(slot_index)
                    .ok_or(engine_world::WorldError::Invariant(
                        "footstep state does not match actor slots",
                    ))?;
            let position = locomotion.centre();
            if !locomotion.grounded()
                || locomotion.state().horizontal().length() < FOOTSTEP_SPEED_MPS
            {
                state.last_position = Some(position);
                continue;
            }
            let Some(previous) = state.last_position.replace(position) else {
                continue;
            };
            let delta = Vec3::new(position.x - previous.x, 0.0, position.z - previous.z).length();
            state.distance_m += delta;
            while state.distance_m >= self.stride_m {
                state.distance_m -= self.stride_m;
                let id = footstep_ids
                    .get(state.variant % footstep_ids.len())
                    .copied()
                    .ok_or(AppError::AudioConfig(
                        "sounds.ron: footstep variants are missing".to_owned(),
                    ))?;
                state.variant = state.variant.wrapping_add(1);
                audio.play(id, Emit::At(position))?;
                self.counts.footsteps = self.counts.footsteps.saturating_add(1);
            }
        }
        Ok(())
    }

    fn sound_id(&self, name: SoundName) -> SoundId {
        self.ids.id(name)
    }
}

fn load_bank(content: &ContentPack) -> Result<(SoundBank, SoundIds, f32), SoundConfigError> {
    let config_path = PathBuf::from(SOUND_CONFIG_PATH);
    let source = content
        .load_text(&AssetPath::new(SOUND_CONFIG_PATH).map_err(|_| {
            SoundConfigError::InvalidField {
                file: config_path.clone(),
                field: "path",
            }
        })?)
        .map_err(|_| SoundConfigError::MissingFile {
            file: config_path.clone(),
            field: "source",
        })?;
    let config: SoundConfig = ron::from_str(source).map_err(|source| SoundConfigError::Parse {
        file: config_path.clone(),
        source,
    })?;
    let (named, stride) = sound_definitions(config)?;
    let contents = named
        .iter()
        .map(|(_, definition)| {
            let path = AssetPath::new(definition.file.to_string_lossy().replace('\\', "/"))
                .map_err(|_| SoundConfigError::InvalidField {
                    file: definition.file.clone(),
                    field: "file",
                })?;
            content
                .load(&path)
                .map(Vec::from)
                .map_err(|_| SoundConfigError::MissingFile {
                    file: definition.file.clone(),
                    field: "file",
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let definitions = named
        .iter()
        .map(|(_, definition)| definition.clone())
        .collect();
    let bank = SoundBank::load_embedded(definitions, contents)?;
    let ids = bank_ids(&bank, &named, &config_path)?;
    Ok((bank, ids, stride))
}

fn sound_definitions(
    config: SoundConfig,
) -> Result<(Vec<(SoundName, SoundDef)>, f32), SoundConfigError> {
    let config_path = PathBuf::from(SOUND_CONFIG_PATH);
    if !config.footstep_stride_m.is_finite() || config.footstep_stride_m <= 0.0 {
        return Err(SoundConfigError::InvalidField {
            file: config_path,
            field: "footstep_stride_m",
        });
    }
    let mut entries = BTreeMap::new();
    for entry in config.sounds {
        let file = PathBuf::from(SOUND_DIRECTORY).join(&entry.file);
        if entries.contains_key(&entry.id) {
            return Err(SoundConfigError::InvalidField {
                file: config_path,
                field: "sounds.id",
            });
        }
        let volume = Volume::new(entry.volume).map_err(|_| SoundConfigError::InvalidField {
            file: file.clone(),
            field: "volume",
        })?;
        let spatial = entry
            .spatial
            .map(|range| {
                Attenuation::new(range.min_distance, range.max_distance).map_err(|_| {
                    SoundConfigError::InvalidField {
                        file: file.clone(),
                        field: "spatial",
                    }
                })
            })
            .transpose()?;
        entries.insert(
            entry.id,
            SoundDef {
                file,
                bus: entry.bus.into(),
                volume,
                max_voices: entry.max_voices,
                priority: entry.priority,
                spatial,
                pitch_jitter: entry.pitch_jitter,
            },
        );
    }
    let mut named_definitions = Vec::with_capacity(SoundName::ALL.len());
    for name in SoundName::ALL {
        let definition = entries
            .remove(&name)
            .ok_or_else(|| SoundConfigError::InvalidField {
                file: config_path.clone(),
                field: "sounds.id",
            })?;
        named_definitions.push((name, definition));
    }
    Ok((named_definitions, config.footstep_stride_m))
}

fn bank_ids(
    bank: &SoundBank,
    named: &[(SoundName, SoundDef)],
    config_path: &std::path::Path,
) -> Result<SoundIds, SoundConfigError> {
    let mut ids = BTreeMap::new();
    for (index, (name, _)) in named.iter().enumerate() {
        let id = bank
            .id(index)
            .ok_or_else(|| SoundConfigError::InvalidField {
                file: config_path.to_path_buf(),
                field: "sounds.id",
            })?;
        ids.insert(*name, id);
    }
    let sound_id = |name| {
        ids.get(&name)
            .copied()
            .ok_or_else(|| SoundConfigError::InvalidField {
                file: config_path.to_path_buf(),
                field: "sounds.id",
            })
    };
    Ok(SoundIds {
        rifle_near: sound_id(SoundName::RifleNear)?,
        rifle_far: sound_id(SoundName::RifleFar)?,
        footstep_1: sound_id(SoundName::Footstep1)?,
        footstep_2: sound_id(SoundName::Footstep2)?,
        footstep_3: sound_id(SoundName::Footstep3)?,
        hit_tick: sound_id(SoundName::HitTick)?,
        kill_confirm: sound_id(SoundName::KillConfirm)?,
        dry_fire: sound_id(SoundName::DryFire)?,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests {
    use super::*;
    use crate::{ImagineGame, app_config};
    use app::{App, ViewAngles, new_mock_audio};
    use glam::Vec2;
    use kit_fps::{MoveCommand, Trigger};
    use platform::{DeviceClass, DeviceProfile};

    fn parsed_config() -> Result<SoundConfig, Box<dyn std::error::Error>> {
        let content = crate::test_support::pack().expect("test pack");
        let path = AssetPath::new(SOUND_CONFIG_PATH)?;
        Ok(ron::from_str(content.load_text(&path)?)?)
    }

    #[test]
    fn travel_discards_per_world_audio_history() -> Result<(), Box<dyn std::error::Error>> {
        let content = crate::test_support::pack().expect("test pack");
        let mut audio = ImagineAudio::new(&content)?;
        audio.footsteps.push(FootstepState {
            last_position: Some(Vec3::X),
            distance_m: 2.0,
            variant: 2,
        });
        audio.last_shots.push(4);
        audio.last_hits = 3;
        audio.last_kills = 2;

        audio.on_travel();

        assert!(audio.footsteps.is_empty());
        assert!(audio.last_shots.is_empty());
        assert_eq!(audio.last_hits, 0);
        assert_eq!(audio.last_kills, 0);
        Ok(())
    }

    #[test]
    fn sounds_config_has_files_and_ranges_in_bounds() -> Result<(), Box<dyn std::error::Error>> {
        let config = parsed_config()?;
        let (definitions, _) = sound_definitions(config)?;
        assert_eq!(definitions.len(), SoundName::COUNT);
        assert_eq!(
            definitions
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            SoundName::ALL
        );
        Ok(())
    }

    #[test]
    fn missing_packed_sound_is_a_typed_error() -> Result<(), Box<dyn std::error::Error>> {
        let content = crate::test_support::pack().expect("test pack");
        let mut entries = content
            .entries()
            .filter(|entry| entry.path.as_str() != "content/audio/placeholder/rifle_near.wav")
            .map(|entry| (entry.path.clone(), entry.bytes.clone()))
            .collect::<Vec<_>>();
        let mut bytes = Vec::new();
        ContentPack::write(std::mem::take(&mut entries), &mut bytes)?;
        let content = engine_assets::PackReader::read(std::io::Cursor::new(bytes))?;
        let error = load_bank(&content).err().ok_or("missing sound must fail")?;
        assert!(matches!(
            error,
            SoundConfigError::MissingFile { file, field: "file" }
                if file.to_string_lossy().contains("rifle_near.wav")
        ));
        Ok(())
    }

    #[test]
    fn bad_spatial_range_error_names_file_and_field() -> Result<(), Box<dyn std::error::Error>> {
        let mut config = parsed_config()?;
        let first = config
            .sounds
            .first_mut()
            .ok_or("near sound entry missing")?;
        first.spatial = Some(SpatialData {
            min_distance: 10.0,
            max_distance: 2.0,
        });
        let error = sound_definitions(config)
            .err()
            .ok_or("bad distance range must fail")?;
        assert!(matches!(
            error,
            SoundConfigError::InvalidField { file, field: "spatial" }
                if file.to_string_lossy().contains("rifle_near.wav")
        ));
        Ok(())
    }

    #[test]
    fn sound_ids_ignore_config_order() -> Result<(), Box<dyn std::error::Error>> {
        let content = crate::test_support::pack().expect("test pack");
        let source = content.load_text(&AssetPath::new(SOUND_CONFIG_PATH)?)?;
        let mut config: SoundConfig = ron::from_str(source)?;
        config.sounds.reverse();
        let (named, _) = sound_definitions(config)?;
        let definitions = named
            .iter()
            .map(|(_, definition)| definition.clone())
            .collect();
        let audio_bytes = named
            .iter()
            .map(|(_, definition)| {
                let path = AssetPath::new(definition.file.to_string_lossy().replace('\\', "/"))?;
                Ok(content.load(&path)?.to_vec())
            })
            .collect::<Result<Vec<_>, engine_assets::PackError>>()?;
        let bank = SoundBank::load_embedded(definitions, audio_bytes)?;
        let ids = bank_ids(&bank, &named, &PathBuf::from(SOUND_CONFIG_PATH))?;
        assert_eq!(bank.len(), SoundName::COUNT);
        assert_eq!(
            ids.id(SoundName::RifleNear),
            bank.id(0).ok_or("near id missing")?
        );
        assert_eq!(
            ids.id(SoundName::DryFire),
            bank.id(7).ok_or("dry fire id missing")?
        );
        Ok(())
    }

    #[test]
    fn duplicate_or_missing_sound_names_report_the_id_field()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut duplicate = parsed_config()?;
        let first = duplicate.sounds.first().cloned().ok_or("sound missing")?;
        duplicate.sounds.push(first);
        assert!(matches!(
            sound_definitions(duplicate),
            Err(SoundConfigError::InvalidField {
                field: "sounds.id",
                ..
            })
        ));

        let mut missing = parsed_config()?;
        missing
            .sounds
            .truncate(missing.sounds.len().saturating_sub(1));
        assert!(matches!(
            sound_definitions(missing),
            Err(SoundConfigError::InvalidField {
                field: "sounds.id",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn six_hundred_tick_sound_counts_repeat_with_a_mock_backend()
    -> Result<(), Box<dyn std::error::Error>> {
        let first = scripted_match_counts()?;
        let second = scripted_match_counts()?;
        assert_eq!(first, second);
        // Four ticks are sampled per frame: counters preserve all shots, and the footstep loop
        // emits every stride crossed by the sampled world displacement (not only one per frame).
        // Recorded after the closer three-lane arena and 25 m bot engagement tuning, and again
        // when shots became kit events (D78): reading the magazine missed a respawned actor's
        // shots until its ammunition fell below the count seen before death, so fewer shots were
        // played and fewer bots were alerted by hearing (and walked fewer strides).
        assert_eq!(
            first,
            AudioPlayCounts {
                shots: 83,
                footsteps: 135,
                hits: 0,
                kills: 0,
            }
        );
        Ok(())
    }

    fn scripted_match_counts() -> Result<AudioPlayCounts, Box<dyn std::error::Error>> {
        let content = crate::test_support::pack().expect("test pack");
        let config = app_config(None)?;
        let profile = DeviceProfile::detect(DeviceClass::Desktop, None, false)?;
        let mut app = App::new(
            ImagineGame::from_content(&content),
            &crate::sim::ImagineSettings::default(),
            &config,
            &profile,
        )?;
        let mut audio = new_mock_audio()?;
        let mut presentation = ImagineAudio::new(&content)?;
        for tick in 0..600_u32 {
            let fire = tick % 24 == 0;
            let command = MoveCommand::new(
                Vec2::new(0.0, 1.0),
                ViewAngles::ZERO,
                false,
                false,
                if fire {
                    Trigger::Held
                } else {
                    Trigger::Released
                },
                false,
            )?;
            crate::sim::set_player_command(app.world_mut(), command)?;
            app.step()?;
            if tick % 4 == 3 {
                presentation.play_frame(app.world(), &mut audio)?;
                presentation.play_frame(app.world(), &mut audio)?;
            }
        }
        Ok(presentation.counts())
    }
}
