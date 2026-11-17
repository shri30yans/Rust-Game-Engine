//! Deterministic, dependency-free synthesis for temporary IMAGINE sound placeholders.
//!
//! Each output is mono IEEE-float WAV at a fixed sample rate. Integer phase and noise state make
//! repeated generation byte-identical on the same platform.

use std::{fs, io, path::Path};

const SAMPLE_RATE: u32 = 44_100;
const MAX_AMPLITUDE: f32 = 0.9;

/// The fixed placeholder sounds emitted by this tool.
pub const OUTPUTS: [&str; 8] = [
    "rifle_near.wav",
    "rifle_far.wav",
    "footstep_1.wav",
    "footstep_2.wav",
    "footstep_3.wav",
    "hit_tick.wav",
    "kill_confirm.wav",
    "dry_fire.wav",
];

/// Creates the deterministic placeholder WAV set in `directory`.
///
/// # Errors
///
/// Returns an I/O error when the directory or any file cannot be written.
pub fn generate(directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    for (index, filename) in OUTPUTS.iter().enumerate() {
        let (duration_ms, kind) = match index {
            0 => (250, Kind::NearShot),
            1 => (600, Kind::FarShot),
            2 => (120, Kind::Foot(0)),
            3 => (120, Kind::Foot(1)),
            4 => (120, Kind::Foot(2)),
            5 => (80, Kind::Hit),
            6 => (280, Kind::Kill),
            _ => (85, Kind::Click),
        };
        let samples = synthesize(duration_ms, kind);
        write_wave(directory.join(filename), &samples)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Kind {
    NearShot,
    FarShot,
    Foot(u32),
    Hit,
    Kill,
    Click,
}

fn synthesize(duration_ms: u32, kind: Kind) -> Vec<f32> {
    let frame_count = SAMPLE_RATE.saturating_mul(duration_ms) / 1_000;
    let mut samples = Vec::with_capacity(usize::try_from(frame_count).unwrap_or(0));
    let mut noise = 0x9e37_79b9_u32;
    let mut low_pass = 0.0_f32;
    for frame in 0..frame_count {
        let time = frame as f32 / SAMPLE_RATE as f32;
        let progress = time / (duration_ms as f32 / 1_000.0);
        let raw = match kind {
            Kind::NearShot => {
                let n = next_noise(&mut noise);
                let burst = n * decay(progress, 0.07);
                let thump =
                    (time * 2_0_f32 * core::f32::consts::PI * 72.0).sin() * decay(progress, 0.19);
                burst * 0.52 + thump * 0.4
            }
            Kind::FarShot => {
                let n = next_noise(&mut noise);
                low_pass += (n - low_pass) * 0.035;
                (low_pass * decay(progress, 0.8)
                    + (time * 2.0 * core::f32::consts::PI * 58.0).sin()
                        * decay(progress, 0.95)
                        * 0.45)
                    * 0.8
            }
            Kind::Foot(variant) => {
                let frequency = 68.0 + variant as f32 * 9.0;
                let n = next_noise(&mut noise);
                low_pass += (n - low_pass) * (0.04 + variant as f32 * 0.01);
                ((time * 2.0 * core::f32::consts::PI * frequency).sin() * 0.68 + low_pass * 0.22)
                    * decay(progress, 0.24)
            }
            Kind::Hit => {
                let frequency = 1_140.0 - progress * 360.0;
                (time * 2.0 * core::f32::consts::PI * frequency).sin()
                    * decay(progress, 0.32)
                    * 0.32
            }
            Kind::Kill => {
                let first = (time * 2.0 * core::f32::consts::PI * 660.0).sin();
                let second = (time * 2.0 * core::f32::consts::PI * 990.0).sin();
                (first * 0.42 + second * 0.34) * decay(progress, 0.75)
            }
            Kind::Click => {
                let n = next_noise(&mut noise);
                (n * 0.35 + (time * 2.0 * core::f32::consts::PI * 1_800.0).sin() * 0.2)
                    * decay(progress, 0.14)
            }
        };
        samples.push(raw.clamp(-MAX_AMPLITUDE, MAX_AMPLITUDE));
    }
    samples
}

fn next_noise(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let centered = (*state >> 8) as f32 / 16_777_216.0;
    centered * 2.0 - 1.0
}

fn decay(progress: f32, rate: f32) -> f32 {
    (-(progress * rate * 8.0)).exp()
}

fn write_wave(path: impl AsRef<Path>, samples: &[f32]) -> io::Result<()> {
    let sample_bytes = u32::try_from(samples.len())
        .ok()
        .and_then(|length| length.checked_mul(4))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "wave sample count is too large",
            )
        })?;
    let riff_size = 36_u32
        .checked_add(sample_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "wave file is too large"))?;
    let mut bytes = Vec::with_capacity(
        usize::try_from(sample_bytes)
            .unwrap_or(0)
            .saturating_add(44),
    );
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&riff_size.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&3_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&32_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&sample_bytes.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    fs::write(path, bytes)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn output_is_deterministic_between_two_runs() -> Result<(), Box<dyn std::error::Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!("sfx-gen-{nonce}"));
        let first = root.join("first");
        let second = root.join("second");
        generate(&first)?;
        generate(&second)?;
        for filename in OUTPUTS {
            assert_eq!(
                fs::read(first.join(filename))?,
                fs::read(second.join(filename))?
            );
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
