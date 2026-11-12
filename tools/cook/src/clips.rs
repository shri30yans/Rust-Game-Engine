//! Samples glTF TRS animation channels at the engine's canonical clip rate.
//!
//! Missing channels retain node rest transforms, and loop state is preserved only when authored.
use std::collections::BTreeMap;

use crate::CookError;
use crate::gltf_read::parse_looping;
use engine_assets::format::Clip;

const CLIP_FPS: f32 = 30.0;

/// Samples all animation channels at the canonical engine clip frequency.
pub(super) fn cook_animations(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    node_map: &BTreeMap<usize, u32>,
) -> Result<Vec<Clip>, CookError> {
    let mut clips = Vec::new();
    for animation in document.animations() {
        let mut source_tracks = BTreeMap::<u32, SourceTrack>::new();
        let mut duration = 0.0_f32;
        for channel in animation.channels() {
            let reader =
                channel.reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()));
            let times = reader
                .read_inputs()
                .ok_or(CookError::Mesh)?
                .collect::<Vec<_>>();
            if let Some(last) = times.last() {
                duration = duration.max(*last);
            }
            let node = *node_map
                .get(&channel.target().node().index())
                .ok_or(CookError::Mesh)?;
            let (translation, rotation, scale) = channel.target().node().transform().decomposed();
            let track = source_tracks.entry(node).or_insert_with(|| SourceTrack {
                rest_translation: translation,
                rest_rotation: rotation,
                rest_scale: scale,
                ..SourceTrack::default()
            });
            let output = reader.read_outputs().ok_or(CookError::Mesh)?;
            match output {
                gltf::animation::util::ReadOutputs::Translations(values) => {
                    track.translations = Some((times, values.collect::<Vec<_>>()))
                }
                gltf::animation::util::ReadOutputs::Scales(values) => {
                    track.scales = Some((times, values.collect::<Vec<_>>()))
                }
                gltf::animation::util::ReadOutputs::Rotations(values) => {
                    track.rotations = Some((times, values.into_f32().collect::<Vec<_>>()))
                }
                gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => {
                    return Err(CookError::Mesh);
                }
            }
        }
        let frame_count = num_traits::ToPrimitive::to_usize(&(duration * CLIP_FPS).round())
            .ok_or(CookError::Mesh)?
            .saturating_add(1);
        let mut channels = Vec::new();
        for (node, source) in source_tracks {
            let mut translations = Vec::with_capacity(frame_count);
            let mut rotations = Vec::with_capacity(frame_count);
            let mut scales = Vec::with_capacity(frame_count);
            for frame in 0..frame_count {
                let time = (num_traits::ToPrimitive::to_f32(&frame).ok_or(CookError::Mesh)?
                    / CLIP_FPS)
                    .min(duration);
                translations.push(
                    sample_vec3(source.translations.as_ref(), time)
                        .unwrap_or(source.rest_translation),
                );
                rotations.push(
                    sample_quat(source.rotations.as_ref(), time).unwrap_or(source.rest_rotation),
                );
                scales.push(sample_vec3(source.scales.as_ref(), time).unwrap_or(source.rest_scale));
            }
            channels.push(engine_assets::format::TrsChannel {
                node,
                translations,
                rotations,
                scales,
            });
        }
        clips.push(Clip {
            name: animation.name().unwrap_or("clip").to_owned(),
            duration,
            fps: CLIP_FPS,
            channels,
            looping: animation.extras().as_deref().and_then(parse_looping),
        });
    }
    Ok(clips)
}

#[derive(Default)]
struct SourceTrack {
    rest_translation: [f32; 3],
    rest_rotation: [f32; 4],
    rest_scale: [f32; 3],
    translations: Option<(Vec<f32>, Vec<[f32; 3]>)>,
    rotations: Option<(Vec<f32>, Vec<[f32; 4]>)>,
    scales: Option<(Vec<f32>, Vec<[f32; 3]>)>,
}

fn sample_vec3(source: Option<&(Vec<f32>, Vec<[f32; 3]>)>, time: f32) -> Option<[f32; 3]> {
    let (times, values) = source?;
    sample_values(times, values, time, |a, b, factor| {
        [
            a[0] + (b[0] - a[0]) * factor,
            a[1] + (b[1] - a[1]) * factor,
            a[2] + (b[2] - a[2]) * factor,
        ]
    })
}

fn sample_quat(source: Option<&(Vec<f32>, Vec<[f32; 4]>)>, time: f32) -> Option<[f32; 4]> {
    let (times, values) = source?;
    sample_values(times, values, time, |a, b, factor| {
        let first = glam::Quat::from_xyzw(a[0], a[1], a[2], a[3]);
        let second = glam::Quat::from_xyzw(b[0], b[1], b[2], b[3]);
        let value = first.slerp(second, factor).normalize();
        [value.x, value.y, value.z, value.w]
    })
}

fn sample_values<T: Copy>(
    times: &[f32],
    values: &[T],
    time: f32,
    interpolate: impl Fn(T, T, f32) -> T,
) -> Option<T> {
    let first = *values.first()?;
    if time <= times.first().copied().unwrap_or(0.0) {
        return Some(first);
    }
    for index in 0..times.len().saturating_sub(1) {
        let start = *times.get(index)?;
        let end = *times.get(index + 1)?;
        if time <= end {
            let left = *values.get(index)?;
            let right = *values.get(index + 1)?;
            let factor = if end <= start {
                0.0
            } else {
                ((time - start) / (end - start)).clamp(0.0, 1.0)
            };
            return Some(interpolate(left, right, factor));
        }
    }
    values.last().copied()
}
