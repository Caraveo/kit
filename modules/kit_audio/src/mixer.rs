//! Decoding sound files and mixing playing voices into stereo.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::sync::Arc;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// A decoded sound: stereo frames at its own sample rate.
#[derive(Debug)]
pub struct Clip {
    pub rate: u32,
    pub frames: Vec<[f32; 2]>,
}

impl Clip {
    pub fn duration(&self) -> f32 {
        self.frames.len() as f32 / self.rate.max(1) as f32
    }
}

/// Decode a sound file (`ext` hints the format: "wav", "mp3", ...).
pub fn decode(bytes: Vec<u8>, ext: &str) -> Result<Clip> {
    let mut hint = Hint::new();
    hint.with_extension(ext);
    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .context("unsupported or damaged sound file")?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .context("the file has no audio")?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("unsupported audio codec")?;
    let mut rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let mut frames = Vec::new();
    let mut samples: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e).context("reading the sound file"),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // A damaged packet: skip it, keep the rest.
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(e).context("decoding the sound file"),
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        let channels = spec.channels.count().max(1);
        let buf = samples.get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, spec));
        if buf.capacity() < decoded.capacity() * channels {
            *buf = SampleBuffer::new(decoded.capacity() as u64, spec);
        }
        buf.copy_interleaved_ref(decoded);
        for f in buf.samples().chunks_exact(channels) {
            frames.push(if channels == 1 { [f[0], f[0]] } else { [f[0], f[1]] });
        }
    }
    if frames.is_empty() {
        bail!("the sound file is empty");
    }
    Ok(Clip { rate, frames })
}

struct Voice {
    clip: Arc<Clip>,
    /// Position in clip frames (fractional for resampling and pitch).
    pos: f64,
    looping: bool,
    pitch: f32,
    /// Target left/right gains, and the gains used last buffer (ramped to avoid clicks).
    target: [f32; 2],
    current: [f32; 2],
    fresh: bool,
}

/// Every voice that's playing, mixed on demand by the audio device (or, on
/// the web, by the page's Web Audio code).
pub struct Mixer {
    pub rate: u32,
    voices: HashMap<u64, Voice>,
    finished: Vec<u64>,
    /// Master volume.
    pub volume: f32,
}

/// Equal-power gains for a volume and pan (-1 … 1).
pub fn pan_gains(volume: f32, pan: f32) -> [f32; 2] {
    let a = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    // Centered = both channels at full volume (not -3 dB), so 2D sounds play as recorded.
    let s = std::f32::consts::SQRT_2;
    [volume * (a.cos() * s).min(1.0), volume * (a.sin() * s).min(1.0)]
}

impl Mixer {
    pub fn new(rate: u32) -> Self {
        Self { rate, voices: HashMap::new(), finished: Vec::new(), volume: 1.0 }
    }

    pub fn play(&mut self, id: u64, clip: Arc<Clip>, volume: f32, pan: f32, pitch: f32, looping: bool) {
        let g = pan_gains(volume, pan);
        self.voices.insert(id, Voice { clip, pos: 0.0, looping, pitch, target: g, current: g, fresh: true });
    }

    /// Change a playing voice (volume/pan ramp over the next buffer).
    pub fn set(&mut self, id: u64, volume: f32, pan: f32, pitch: f32, looping: bool) {
        if let Some(v) = self.voices.get_mut(&id) {
            v.target = pan_gains(volume, pan);
            v.pitch = pitch;
            v.looping = looping;
        }
    }

    pub fn stop(&mut self, id: u64) {
        self.voices.remove(&id);
    }

    pub fn stop_all(&mut self) {
        self.voices.clear();
        self.finished.clear();
    }

    pub fn is_playing(&self, id: u64) -> bool {
        self.voices.contains_key(&id)
    }

    /// Voices that reached their end since the last call.
    pub fn take_finished(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.finished)
    }

    /// Mix into interleaved stereo `out` (overwrites it).
    pub fn mix(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        if frames == 0 {
            return;
        }
        let master = self.volume;
        let rate = self.rate.max(1) as f64;
        let mut done = Vec::new();
        for (&id, v) in &mut self.voices {
            let len = v.clip.frames.len();
            let step = v.pitch.max(0.01) as f64 * v.clip.rate as f64 / rate;
            let start = if v.fresh { v.target } else { v.current };
            v.fresh = false;
            let mut ended = false;
            for i in 0..frames {
                if v.pos >= len as f64 {
                    if v.looping {
                        v.pos %= len as f64;
                    } else {
                        ended = true;
                        break;
                    }
                }
                let i0 = v.pos as usize;
                let t = (v.pos - i0 as f64) as f32;
                let i1 = if i0 + 1 < len {
                    i0 + 1
                } else if v.looping {
                    0
                } else {
                    i0
                };
                let (a, b) = (v.clip.frames[i0], v.clip.frames[i1]);
                let k = (i + 1) as f32 / frames as f32;
                let gl = start[0] + (v.target[0] - start[0]) * k;
                let gr = start[1] + (v.target[1] - start[1]) * k;
                out[i * 2] += (a[0] + (b[0] - a[0]) * t) * gl * master;
                out[i * 2 + 1] += (a[1] + (b[1] - a[1]) * t) * gr * master;
                v.pos += step;
            }
            v.current = v.target;
            if ended {
                done.push(id);
            }
        }
        for id in done {
            self.voices.remove(&id);
            self.finished.push(id);
        }
        // Soft clip so many loud sounds don't crackle.
        for s in out.iter_mut() {
            if s.abs() > 0.9 {
                *s = s.signum() * (0.9 + (s.abs() - 0.9).tanh() * 0.1);
            }
        }
    }
}

/// A WAV file in memory (16-bit PCM), for tests and generated sounds.
pub fn wav_bytes(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, seconds: f32) -> Vec<i16> {
        (0..(rate as f32 * seconds) as usize)
            .map(|i| ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12000.0) as i16)
            .collect()
    }

    #[test]
    fn decodes_wav_to_stereo() {
        let clip = decode(wav_bytes(22_050, 1, &tone(22_050, 0.5)), "wav").unwrap();
        assert_eq!(clip.rate, 22_050);
        assert_eq!(clip.frames.len(), 11_025);
        assert!(clip.frames.iter().all(|f| f[0] == f[1]), "mono is copied to both channels");
        assert!(decode(b"not audio".to_vec(), "wav").is_err());
    }

    #[test]
    fn mixes_resamples_pans_and_finishes() {
        let clip = Arc::new(decode(wav_bytes(24_000, 1, &tone(24_000, 0.25)), "wav").unwrap());
        let mut m = Mixer::new(48_000);
        m.play(1, clip.clone(), 1.0, 1.0, 1.0, false); // hard right
        let mut out = vec![0.0; 2 * 4800];
        m.mix(&mut out);
        let (l, r): (f32, f32) = out.chunks(2).fold((0.0, 0.0), |(l, r), f| (l + f[0].abs(), r + f[1].abs()));
        assert!(r > 100.0 && l < r * 0.01, "panned right: l={l} r={r}");
        // 0.25 s at 24 kHz resampled to 48 kHz = 12000 output frames; 4800 done, 7200 left.
        let mut rest = vec![0.0; 2 * 8000];
        m.mix(&mut rest);
        assert!(!m.is_playing(1));
        assert_eq!(m.take_finished(), [1]);
        // Looping voices keep going.
        m.play(2, clip, 0.5, 0.0, 2.0, true);
        m.mix(&mut rest);
        m.mix(&mut rest);
        assert!(m.is_playing(2) && rest.iter().any(|s| s.abs() > 0.1));
        m.stop_all();
        m.mix(&mut rest);
        assert!(rest.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn centered_sounds_play_at_full_volume() {
        let [l, r] = pan_gains(1.0, 0.0);
        assert!((l - 1.0).abs() < 1e-4 && (r - 1.0).abs() < 1e-4);
        let [l, r] = pan_gains(0.5, -1.0);
        assert!((l - 0.5).abs() < 1e-4 && r.abs() < 1e-4);
    }
}
