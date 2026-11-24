//! The computer's audio output, pulling samples from the [`Mixer`].
//!
//! Linux talks to ALSA's `default` device (PipeWire and PulseAudio provide
//! it), loading `libasound.so.2` at runtime like Vulkan, so building Kit needs
//! no audio dev packages. Windows and macOS use cpal (WASAPI / CoreAudio).

use crate::mixer::Mixer;
use anyhow::Result;
use std::sync::{Arc, Mutex};

/// Keeps the output running; dropping it stops the sound.
pub struct Output {
    _inner: Box<dyn std::any::Any>,
    pub rate: u32,
}

#[cfg(all(feature = "device", target_os = "linux"))]
pub fn open(mixer: Arc<Mutex<Mixer>>) -> Result<Output> {
    linux::open(mixer)
}

#[cfg(all(feature = "device", any(windows, target_os = "macos")))]
pub fn open(mixer: Arc<Mutex<Mixer>>) -> Result<Output> {
    native::open(mixer)
}

#[cfg(not(all(feature = "device", any(target_os = "linux", windows, target_os = "macos"))))]
pub fn open(_mixer: Arc<Mutex<Mixer>>) -> Result<Output> {
    anyhow::bail!("no audio output on this platform")
}

#[cfg(all(feature = "device", target_os = "linux"))]
mod linux {
    use super::*;
    use anyhow::{Context, bail};
    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
    use std::sync::atomic::{AtomicBool, Ordering};

    type Pcm = c_void;
    const PLAYBACK: c_int = 0;
    const FORMAT_FLOAT_LE: c_int = 14;
    const FORMAT_S16_LE: c_int = 2;
    const ACCESS_RW_INTERLEAVED: c_int = 3;
    const RATE: u32 = 48_000;
    /// Frames per write (~10 ms).
    const CHUNK: usize = 480;

    struct Alsa {
        _lib: libloading::Library,
        close: unsafe extern "C" fn(*mut Pcm) -> c_int,
        writei: unsafe extern "C" fn(*mut Pcm, *const c_void, c_ulong) -> c_long,
        recover: unsafe extern "C" fn(*mut Pcm, c_int, c_int) -> c_int,
    }

    struct Running {
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    pub fn open(mixer: Arc<Mutex<Mixer>>) -> Result<Output> {
        let lib = unsafe { libloading::Library::new("libasound.so.2") }.context("ALSA (libasound.so.2) not found")?;
        let (pcm, float, alsa) = unsafe {
            let open: libloading::Symbol<unsafe extern "C" fn(*mut *mut Pcm, *const c_char, c_int, c_int) -> c_int> =
                lib.get(b"snd_pcm_open\0")?;
            let set_params: libloading::Symbol<
                unsafe extern "C" fn(*mut Pcm, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int,
            > = lib.get(b"snd_pcm_set_params\0")?;
            let close = *lib.get::<unsafe extern "C" fn(*mut Pcm) -> c_int>(b"snd_pcm_close\0")?;
            let writei = *lib.get(b"snd_pcm_writei\0")?;
            let recover = *lib.get(b"snd_pcm_recover\0")?;
            let mut pcm: *mut Pcm = std::ptr::null_mut();
            if open(&mut pcm, c"default".as_ptr(), PLAYBACK, 0) < 0 || pcm.is_null() {
                bail!("no audio output device");
            }
            // 40 ms latency: responsive, but safe from crackles.
            let mut float = true;
            if set_params(pcm, FORMAT_FLOAT_LE, ACCESS_RW_INTERLEAVED, 2, RATE, 1, 40_000) < 0 {
                float = false;
                if set_params(pcm, FORMAT_S16_LE, ACCESS_RW_INTERLEAVED, 2, RATE, 1, 40_000) < 0 {
                    close(pcm);
                    bail!("the audio device doesn't accept 48 kHz stereo");
                }
            }
            (pcm as usize, float, Alsa { close, writei, recover, _lib: lib })
        };
        mixer.lock().unwrap().rate = RATE;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new().name("kit-audio".into()).spawn(move || {
            let pcm = pcm as *mut Pcm;
            let mut buf = vec![0.0f32; CHUNK * 2];
            let mut ints = vec![0i16; CHUNK * 2];
            while !flag.load(Ordering::Relaxed) {
                if let Ok(mut m) = mixer.lock() {
                    m.mix(&mut buf);
                }
                let ptr = if float {
                    buf.as_ptr() as *const c_void
                } else {
                    for (i, s) in buf.iter().enumerate() {
                        ints[i] = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                    ints.as_ptr() as *const c_void
                };
                // Blocks until the device has room: this paces the loop.
                let n = unsafe { (alsa.writei)(pcm, ptr, CHUNK as c_ulong) };
                if n < 0 && unsafe { (alsa.recover)(pcm, n as c_int, 1) } < 0 {
                    log::warn!("audio output stopped (ALSA error {n})");
                    break;
                }
            }
            unsafe { (alsa.close)(pcm) };
        })?;
        Ok(Output { _inner: Box::new(Running { stop, thread: Some(thread) }), rate: RATE })
    }
}

#[cfg(all(feature = "device", any(windows, target_os = "macos")))]
mod native {
    use super::*;
    use anyhow::Context;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample};

    fn stream<T: SizedSample + FromSample<f32>>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        mixer: Arc<Mutex<Mixer>>,
    ) -> Result<cpal::Stream> {
        let channels = config.channels as usize;
        let mut stereo = Vec::new();
        let stream = device.build_output_stream(
            config,
            move |data: &mut [T], _| {
                let frames = data.len() / channels.max(1);
                stereo.resize(frames * 2, 0.0);
                if let Ok(mut m) = mixer.lock() {
                    m.mix(&mut stereo);
                } else {
                    stereo.fill(0.0);
                }
                for (i, frame) in data.chunks_mut(channels.max(1)).enumerate() {
                    for (c, s) in frame.iter_mut().enumerate() {
                        // Stereo to the first two speakers; others get the mono mix.
                        let v = match c {
                            0 => stereo[i * 2],
                            1 => stereo[i * 2 + 1],
                            _ => (stereo[i * 2] + stereo[i * 2 + 1]) * 0.5,
                        };
                        *s = T::from_sample(v);
                    }
                }
            },
            |e| log::warn!("audio output: {e}"),
            None,
        )?;
        Ok(stream)
    }

    pub fn open(mixer: Arc<Mutex<Mixer>>) -> Result<Output> {
        let device = cpal::default_host().default_output_device().context("no audio output device")?;
        let supported = device.default_output_config()?;
        let rate = supported.sample_rate().0;
        let config: cpal::StreamConfig = supported.config();
        mixer.lock().unwrap().rate = rate;
        let s = match supported.sample_format() {
            SampleFormat::F32 => stream::<f32>(&device, &config, mixer)?,
            SampleFormat::I16 => stream::<i16>(&device, &config, mixer)?,
            SampleFormat::U16 => stream::<u16>(&device, &config, mixer)?,
            SampleFormat::I32 => stream::<i32>(&device, &config, mixer)?,
            other => anyhow::bail!("unsupported audio sample format {other:?}"),
        };
        s.play()?;
        Ok(Output { _inner: Box::new(s), rate })
    }
}
