//! Playback tests on Kira's mock backend: the voice limit, the global cap and culling.
//!
//! These read the voice list directly, which is why they sit inside `audio`.

use std::{fs, io::Write, path::PathBuf};

use kira::backend::mock::MockBackendSettings;

use super::Audio;
use crate::{AudioError, Bus, Emit, SoundBank, SoundDef, SoundId, Volume};

#[test]
fn unknown_sound_id_is_a_typed_error() -> Result<(), Box<dyn std::error::Error>> {
    let mut audio = Audio::<kira::backend::mock::MockBackend>::new(MockBackendSettings::default())?;
    assert!(matches!(
        audio.play(SoundId(0), Emit::Ui),
        Err(AudioError::UnknownSound(0))
    ));
    Ok(())
}

fn sound_file(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!("engine-audio-{name}-{}.wav", std::process::id()));
    let mut file = fs::File::create(&path)?;
    let data_bytes = 882_u32;
    file.write_all(b"RIFF")?;
    file.write_all(&(36_u32 + data_bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&44_100_u32.to_le_bytes())?;
    file.write_all(&88_200_u32.to_le_bytes())?;
    file.write_all(&2_u16.to_le_bytes())?;
    file.write_all(&16_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    for _ in 0..441 {
        file.write_all(&0_i16.to_le_bytes())?;
    }
    Ok(path)
}

fn definition(path: PathBuf, priority: i32, max_voices: usize) -> SoundDef {
    SoundDef {
        file: path,
        bus: Bus::Weapons,
        volume: Volume(1.0),
        max_voices,
        priority,
        spatial: None,
        pitch_jitter: 0.0,
    }
}

#[test]
fn mock_backend_plays_and_applies_voice_limits() -> Result<(), Box<dyn std::error::Error>> {
    let path = sound_file("voice-limit")?;
    let bank = SoundBank::load(vec![definition(path.clone(), 1, 1)])?;
    let mut audio = Audio::<kira::backend::mock::MockBackend>::new(MockBackendSettings::default())?;
    audio.set_bank(bank)?;
    let id = audio.bank.id(0).ok_or("bank entry missing")?;
    audio.play(id, Emit::Ui)?;
    assert_eq!(audio.active_voices(), 1);
    assert_eq!(audio.voices.first().map(|voice| voice.id), Some(id));
    assert_eq!(audio.voices.first().map(|voice| voice.started), Some(1));
    audio.play(id, Emit::Ui)?;
    assert_eq!(audio.active_voices(), 1);
    assert_eq!(audio.voices.first().map(|voice| voice.started), Some(2));
    fs::remove_file(path)?;
    Ok(())
}

#[test]
fn mock_backend_respects_the_global_voice_cap() -> Result<(), Box<dyn std::error::Error>> {
    let first_path = sound_file("global-a")?;
    let second_path = sound_file("global-b")?;
    let bank = SoundBank::load(vec![
        definition(first_path.clone(), 1, 1),
        definition(second_path.clone(), 2, 1),
    ])?;
    let mut audio = Audio::<kira::backend::mock::MockBackend>::with_voice_limit(
        MockBackendSettings::default(),
        1,
    )?;
    audio.set_bank(bank)?;
    let first = audio.bank.id(0).ok_or("first bank entry missing")?;
    let second = audio.bank.id(1).ok_or("second bank entry missing")?;
    audio.play(first, Emit::Ui)?;
    audio.play(second, Emit::Ui)?;
    assert_eq!(audio.active_voices(), 1);
    assert_eq!(audio.voices.first().map(|voice| voice.id), Some(second));
    fs::remove_file(first_path)?;
    fs::remove_file(second_path)?;
    Ok(())
}

#[test]
fn lower_priority_plays_are_culled_when_the_global_voice_cap_is_full()
-> Result<(), Box<dyn std::error::Error>> {
    let loud_path = sound_file("cull-loud")?;
    let quiet_path = sound_file("cull-quiet")?;
    let bank = SoundBank::load(vec![
        definition(loud_path.clone(), 8, 1),
        definition(quiet_path.clone(), 2, 1),
    ])?;
    let mut audio = Audio::<kira::backend::mock::MockBackend>::with_voice_limit(
        MockBackendSettings::default(),
        1,
    )?;
    audio.set_bank(bank)?;
    let loud = audio.bank.id(0).ok_or("loud sound missing")?;
    let quiet = audio.bank.id(1).ok_or("quiet sound missing")?;
    audio.play(loud, Emit::Ui)?;
    audio.play(quiet, Emit::Ui)?;
    assert_eq!(audio.active_voices(), 1);
    assert_eq!(audio.voices.first().map(|voice| voice.id), Some(loud));
    assert_eq!(audio.culled_plays(), 1);
    fs::remove_file(loud_path)?;
    fs::remove_file(quiet_path)?;
    Ok(())
}
