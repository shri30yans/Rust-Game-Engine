//! Reports allocations made by one warmed Kira mock-backend playback.
//!
//! The spatial tracks are installed with the bank before measurement, so per-play sub-track
//! creation is excluded. Kira 0.12.6 measured 11 allocations and 7,304 bytes for static sound
//! insertion and command handles here; the assertion leaves one allocation and 888 bytes margin.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use engine_audio::{Attenuation, Audio, Bus, Emit, SoundBank, SoundDef, Volume};
use kira::backend::mock::{MockBackend, MockBackendSettings};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use std::{alloc::System, fs, io::Write, path::PathBuf};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[test]
fn reports_warmed_mock_playback_allocations() -> Result<(), Box<dyn std::error::Error>> {
    let path = write_test_wave()?;
    let bank = SoundBank::load(vec![SoundDef {
        file: path.clone(),
        bus: Bus::Weapons,
        volume: Volume::new(1.0)?,
        max_voices: 4,
        priority: 1,
        spatial: Some(Attenuation::new(1.0, 20.0)?),
        pitch_jitter: 0.0,
    }])?;
    let id = bank.id(0).ok_or("test sound was not loaded")?;
    let mut audio = Audio::<MockBackend>::with_voice_limit(MockBackendSettings::default(), 4)?;
    audio.set_bank(bank)?;
    audio.play(id, Emit::At(glam::Vec3::ZERO))?;

    let region = Region::new(GLOBAL);
    audio.play(id, Emit::At(glam::Vec3::X))?;
    let stats = region.change();
    println!("warmed pooled spatial play allocation measurement: {stats:?}");
    assert!(
        stats.allocations <= 12,
        "Kira playback allocation bound: {stats:?}"
    );
    assert!(
        stats.bytes_allocated <= 8_192,
        "Kira playback byte bound: {stats:?}"
    );

    fs::remove_file(path)?;
    Ok(())
}

fn write_test_wave() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "engine-audio-allocation-{}.wav",
        std::process::id()
    ));
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
