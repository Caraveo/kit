//! Sound components and the math for proximity (3D) audio. Playback lives in
//! the `kit_audio` module; scripts and the editor talk to it through
//! [`AudioCommands`] and read [`AudioState`].

use glam::Vec3;
use kit_ecs::Entity;
use serde::{Deserialize, Serialize};

/// Sound files Kit can play (open formats the decoder supports).
pub const SOUND_EXTENSIONS: &[&str] = &["wav", "mp3", "ogg", "oga", "flac", "aac", "m4a"];

pub fn is_sound(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SOUND_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// How a 3D sound fades between its min and max distance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rolloff {
    /// Even fade from full volume to silence.
    Linear,
    /// Natural: drops quickly near the source, then tails off (like real sound).
    #[default]
    Inverse,
    /// Stays loud, then drops off steeply near the max distance.
    Smooth,
}

impl Rolloff {
    pub const ALL: [Rolloff; 3] = [Rolloff::Inverse, Rolloff::Linear, Rolloff::Smooth];

    pub fn label(self) -> &'static str {
        match self {
            Rolloff::Linear => "Linear",
            Rolloff::Inverse => "Natural",
            Rolloff::Smooth => "Smooth",
        }
    }
}

/// Plays a sound file from this object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSource {
    /// Project-relative sound file (`sounds/door.wav`).
    pub clip: String,
    /// 0 = silent, 1 = as recorded (can go above 1).
    pub volume: f32,
    /// Playback speed: 2 = an octave up and twice as fast.
    pub pitch: f32,
    pub looping: bool,
    /// Start playing when the game starts.
    pub play_on_start: bool,
    /// Proximity: louder near the listener (the active camera), panned left/right.
    pub spatial: bool,
    /// Full volume within this distance.
    pub min_distance: f32,
    /// Silent beyond this distance.
    pub max_distance: f32,
    pub rolloff: Rolloff,
}

impl Default for AudioSource {
    fn default() -> Self {
        Self {
            clip: String::new(),
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            play_on_start: true,
            spatial: false,
            min_distance: 1.0,
            max_distance: 20.0,
            rolloff: Rolloff::Inverse,
        }
    }
}

impl AudioSource {
    /// Volume factor for a listener `distance` away (1 for non-spatial sounds).
    pub fn attenuation(&self, distance: f32) -> f32 {
        if !self.spatial {
            return 1.0;
        }
        let min = self.min_distance.max(0.01);
        let max = self.max_distance.max(min + 0.01);
        if distance <= min {
            return 1.0;
        }
        if distance >= max {
            return 0.0;
        }
        let t = (distance - min) / (max - min);
        match self.rolloff {
            Rolloff::Linear => 1.0 - t,
            // 1/distance, shifted so it reaches exactly 0 at max.
            Rolloff::Inverse => ((min / distance) - (min / max)) / (1.0 - min / max),
            Rolloff::Smooth => {
                let s = 1.0 - t;
                s * s * (3.0 - 2.0 * s)
            }
        }
        .clamp(0.0, 1.0)
    }
}

/// Stereo pan (-1 left … 1 right) of a sound at `source` for a listener at
/// `listener` whose right-hand direction is `right`.
pub fn stereo_pan(listener: Vec3, right: Vec3, source: Vec3) -> f32 {
    let to = source - listener;
    let d = to.length();
    if d < 1e-4 {
        return 0.0;
    }
    // Close sounds stay centered a little; distant ones pan fully.
    (to.dot(right.normalize_or_zero()) / d * 0.85).clamp(-1.0, 1.0)
}

/// A request for the audio module (from scripts or the editor).
#[derive(Clone, Debug, PartialEq)]
pub enum AudioCommand {
    /// Play an entity's AudioSource (restarts it if playing).
    Play(Entity),
    Stop(Entity),
    /// Fire-and-forget: play a sound file once at a position (`None` = not spatial).
    OneShot { clip: String, volume: f32, pitch: f32, at: Option<(Vec3, f32, f32)> },
    StopAll,
}

/// Pending audio requests, drained by the audio module each frame.
#[derive(Default, Debug)]
pub struct AudioCommands(pub Vec<AudioCommand>);

/// What the audio module is playing (read by scripts: `Audio.Playing`).
#[derive(Default, Debug)]
pub struct AudioState {
    pub playing: std::collections::HashSet<Entity>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proximity_fades_between_min_and_max() {
        let mut s = AudioSource { spatial: true, min_distance: 2.0, max_distance: 10.0, ..Default::default() };
        for rolloff in Rolloff::ALL {
            s.rolloff = rolloff;
            assert_eq!(s.attenuation(1.0), 1.0, "{rolloff:?}: full volume inside min");
            assert_eq!(s.attenuation(10.0), 0.0, "{rolloff:?}: silent at max");
            let (a, b) = (s.attenuation(4.0), s.attenuation(8.0));
            assert!(a > b && a < 1.0 && b > 0.0, "{rolloff:?}: fades {a} > {b}");
        }
        s.spatial = false;
        assert_eq!(s.attenuation(1000.0), 1.0, "2D sounds ignore distance");
    }

    #[test]
    fn pan_follows_the_listeners_right() {
        let right = Vec3::X;
        assert!(stereo_pan(Vec3::ZERO, right, Vec3::new(5.0, 0.0, 0.0)) > 0.8);
        assert!(stereo_pan(Vec3::ZERO, right, Vec3::new(-5.0, 0.0, 0.0)) < -0.8);
        assert!(stereo_pan(Vec3::ZERO, right, Vec3::new(0.0, 0.0, -5.0)).abs() < 1e-5);
        assert!(is_sound("sounds/Door.WAV") && is_sound("a.m4a") && !is_sound("a.png"));
    }
}
