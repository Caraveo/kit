//! Animation clips: keyed volume poses, sampled over time.

use kit_gamesys::{RigClip, RigKey, RigTrack};
use kit_scene::Transform;

/// The pose of one track at `time` (clamped; keys must be sorted by time).
pub fn sample(track: &RigTrack, time: f32) -> Option<Transform> {
    let keys = &track.keys;
    let first = keys.first()?;
    if keys.len() == 1 || time <= first.time {
        return Some(transform(first));
    }
    let last = keys.last().unwrap();
    if time >= last.time {
        return Some(transform(last));
    }
    let i = keys.partition_point(|k| k.time <= time);
    let (a, b) = (&keys[i - 1], &keys[i]);
    let t = ((time - a.time) / (b.time - a.time).max(1e-6)).clamp(0.0, 1.0);
    // Smooth in and out between keys.
    let t = t * t * (3.0 - 2.0 * t);
    Some(Transform {
        translation: a.translation.lerp(b.translation, t),
        rotation: a.rotation.slerp(b.rotation, t).normalize(),
        scale: a.scale.lerp(b.scale, t),
    })
}

fn transform(k: &RigKey) -> Transform {
    Transform { translation: k.translation, rotation: k.rotation, scale: k.scale }
}

/// Clip time for playback `elapsed` seconds after it started.
pub fn clip_time(clip: &RigClip, elapsed: f32) -> f32 {
    let len = clip.length.max(1e-3);
    if clip.looping { elapsed.rem_euclid(len) } else { elapsed.min(len) }
}

/// Set (or replace) a key for `volume` at `time`.
pub fn set_key(clip: &mut RigClip, volume: &str, time: f32, t: &Transform) {
    let key = RigKey { time, translation: t.translation, rotation: t.rotation, scale: t.scale };
    let track = match clip.tracks.iter_mut().position(|tr| tr.volume == volume) {
        Some(i) => &mut clip.tracks[i],
        None => {
            clip.tracks.push(RigTrack { volume: volume.to_owned(), keys: Vec::new() });
            clip.tracks.last_mut().unwrap()
        }
    };
    match track.keys.iter_mut().find(|k| (k.time - time).abs() < 1e-3) {
        Some(k) => *k = key,
        None => {
            track.keys.push(key);
            track.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
        }
    }
}

/// Remove every key at `time`.
pub fn remove_keys_at(clip: &mut RigClip, time: f32) -> usize {
    let mut n = 0;
    for tr in &mut clip.tracks {
        let before = tr.keys.len();
        tr.keys.retain(|k| (k.time - time).abs() >= 1e-3);
        n += before - tr.keys.len();
    }
    clip.tracks.retain(|t| !t.keys.is_empty());
    n
}

/// All key times in a clip (sorted, unique).
pub fn key_times(clip: &RigClip) -> Vec<f32> {
    let mut times: Vec<f32> = clip.tracks.iter().flat_map(|t| t.keys.iter().map(|k| k.time)).collect();
    times.sort_by(f32::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    times
}
