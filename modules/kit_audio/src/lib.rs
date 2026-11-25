//! # kit_audio
//!
//! Plays sounds (Defold's `sound` component): every [`AudioSource`] on an
//! object, one-shot sounds from scripts, and the editor's previews. Spatial
//! sources fade with distance from the listener (the active camera) and pan
//! left/right; see [`AudioSource::attenuation`].
//!
//! Sound files are decoded once (WAV, MP3, Ogg Vorbis, FLAC, AAC/M4A) and
//! mixed by [`Mixer`]. On desktop the mixer feeds the audio device; in the web
//! player the page pulls mixed samples for Web Audio.

pub mod device;
pub mod mixer;

pub use mixer::{Clip, Mixer, decode, wav_bytes};

use glam::Vec3;
use kit_core::{App, Module, Stage};
use kit_ecs::{Entity, World};
use kit_gamesys::{AudioCommand, AudioCommands, AudioSource, AudioState, stereo_pan};
use kit_resource::Resources;
use kit_scene::GlobalTransform;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Voice id for the editor's clip preview.
const PREVIEW: u64 = 0;

/// The audio module's state: decoded clips and who's playing what.
pub struct Audio {
    pub mixer: Arc<Mutex<Mixer>>,
    output: Option<device::Output>,
    /// Open the audio device the first time something plays.
    wants_device: bool,
    tried_device: bool,
    clips: HashMap<String, Option<Arc<Clip>>>,
    /// Entity → its voice.
    voices: HashMap<Entity, u64>,
    next_id: u64,
    /// Play mode started (play-on-start sources were triggered).
    started: bool,
}

impl Audio {
    /// A silent audio system: nothing is sent to a device (tests, the web player).
    pub fn without_device() -> Self {
        Self {
            mixer: Arc::new(Mutex::new(Mixer::new(48_000))),
            output: None,
            wants_device: false,
            tried_device: false,
            clips: HashMap::new(),
            voices: HashMap::new(),
            next_id: 1,
            started: false,
        }
    }

    /// Play through the computer's audio output, opened the first time
    /// something plays (silent, with a warning, if there's none).
    pub fn with_device() -> Self {
        Self { wants_device: true, ..Self::without_device() }
    }

    fn ensure_output(&mut self) {
        if !self.wants_device || self.tried_device {
            return;
        }
        self.tried_device = true;
        match device::open(self.mixer.clone()) {
            Ok(out) => {
                log::info!("audio output: {} Hz", out.rate);
                self.output = Some(out);
            }
            Err(e) => log::warn!("sound is off: {e:#}"),
        }
    }

    /// Is sound reaching speakers? (Opens the device if it wasn't yet.)
    pub fn has_output(&mut self) -> bool {
        self.ensure_output();
        self.output.is_some()
    }

    /// Decode a clip (cached; errors are reported once).
    pub fn clip(&mut self, resources: &Resources, path: &str) -> anyhow::Result<Arc<Clip>> {
        if let Some(c) = self.clips.get(path) {
            return c.clone().ok_or_else(|| anyhow::anyhow!("{path} couldn't be played"));
        }
        let ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let result = resources.read_bytes(path).and_then(|b| decode(b, &ext)).map(Arc::new);
        self.clips.insert(path.to_owned(), result.as_ref().ok().cloned());
        result.map_err(|e| anyhow::anyhow!("{path}: {e:#}"))
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Editor: play a clip once, not spatial, replacing the previous preview.
    pub fn preview(&mut self, resources: &Resources, path: &str, volume: f32, pitch: f32) -> anyhow::Result<f32> {
        let clip = self.clip(resources, path)?;
        self.ensure_output();
        let seconds = clip.duration() / pitch.max(0.01);
        self.mixer.lock().unwrap().play(PREVIEW, clip, volume, 0.0, pitch, false);
        Ok(seconds)
    }

    pub fn stop_preview(&mut self) {
        self.mixer.lock().unwrap().stop(PREVIEW);
    }

    pub fn previewing(&self) -> bool {
        self.mixer.lock().unwrap().is_playing(PREVIEW)
    }

    /// Forget decoded clips (files changed on disk).
    pub fn clear_cache(&mut self) {
        self.clips.clear();
    }
}

/// Where the listener is: the active camera (position, right direction).
fn listener(world: &World) -> (Vec3, Vec3) {
    kit_gamesys::active_camera(world)
        .and_then(|e| world.get::<GlobalTransform>(e).map(|g| g.0))
        .map(|m| (m.w_axis.truncate(), m.x_axis.truncate().normalize_or(Vec3::X)))
        .unwrap_or((Vec3::ZERO, Vec3::X))
}

/// Volume and pan for a source at `pos`.
fn mix_params(source: &AudioSource, pos: Vec3, listener: (Vec3, Vec3)) -> (f32, f32) {
    if !source.spatial {
        return (source.volume.max(0.0), 0.0);
    }
    let gain = source.volume.max(0.0) * source.attenuation(pos.distance(listener.0));
    (gain, stereo_pan(listener.0, listener.1, pos))
}

fn report(world: &World, text: String) {
    if let Some(mut c) = world.try_resource_mut::<kit_core::Console>() {
        c.error("audio", text);
    }
}

fn play_entity(world: &World, audio: &mut Audio, e: Entity, l: (Vec3, Vec3)) {
    let Some(source) = world.get_cloned::<AudioSource>(e) else { return };
    if source.clip.trim().is_empty() {
        return;
    }
    let Some(resources) = world.try_resource::<Resources>().map(|r| r.clone()) else { return };
    match audio.clip(&resources, &source.clip) {
        Ok(clip) => {
            let pos = world.get::<GlobalTransform>(e).map(|g| g.0.w_axis.truncate()).unwrap_or_default();
            let (gain, pan) = mix_params(&source, pos, l);
            let id = audio.voices.get(&e).copied().unwrap_or_else(|| audio.id());
            audio.mixer.lock().unwrap().play(id, clip, gain, pan, source.pitch, source.looping);
            audio.voices.insert(e, id);
        }
        Err(e) => report(world, format!("{e:#}")),
    }
}

/// Play-mode system: start sources, run commands, update proximity.
pub fn update_audio(world: &mut World) {
    let Some(mut audio) = world.try_resource_mut::<Audio>() else { return };
    let world = &*world;
    let l = listener(world);
    if !audio.started {
        audio.started = true;
        if world.storage::<AudioSource>().is_some_and(|s| s.iter().next().is_some()) {
            audio.ensure_output();
        }
        // Decode everything up front so the first play doesn't hitch.
        if let (Some(sources), Some(resources)) = (world.storage::<AudioSource>(), world.try_resource::<Resources>()) {
            let clips: Vec<String> = sources.iter().map(|(_, s)| s.clip.clone()).filter(|c| !c.is_empty()).collect();
            for clip in clips {
                let _ = audio.clip(&resources, &clip);
            }
        }
        for e in world.query::<(AudioSource,)>() {
            if world.get::<AudioSource>(e).is_some_and(|s| s.play_on_start) {
                play_entity(world, &mut audio, e, l);
            }
        }
    }
    let commands = world.try_resource_mut::<AudioCommands>().map(|mut c| std::mem::take(&mut c.0)).unwrap_or_default();
    for cmd in commands {
        match cmd {
            AudioCommand::Play(e) => play_entity(world, &mut audio, e, l),
            AudioCommand::Stop(e) => {
                if let Some(id) = audio.voices.remove(&e) {
                    audio.mixer.lock().unwrap().stop(id);
                }
            }
            AudioCommand::OneShot { clip, volume, pitch, at } => {
                audio.ensure_output();
                let Some(resources) = world.try_resource::<Resources>().map(|r| r.clone()) else { continue };
                match audio.clip(&resources, &clip) {
                    Ok(c) => {
                        let source = AudioSource {
                            volume,
                            spatial: at.is_some(),
                            min_distance: at.map_or(1.0, |a| a.1),
                            max_distance: at.map_or(20.0, |a| a.2),
                            ..Default::default()
                        };
                        let (gain, pan) = mix_params(&source, at.map_or(l.0, |a| a.0), l);
                        let id = audio.id();
                        audio.mixer.lock().unwrap().play(id, c, gain, pan, pitch, false);
                    }
                    Err(e) => report(world, format!("{e:#}")),
                }
            }
            AudioCommand::StopAll => {
                audio.voices.clear();
                audio.mixer.lock().unwrap().stop_all();
            }
        }
    }
    // Follow moving sources and listener; drop voices of removed objects.
    let mixer = audio.mixer.clone();
    let mut m = mixer.lock().unwrap();
    let finished = m.take_finished();
    audio.voices.retain(|e, id| {
        if finished.contains(id) || !m.is_playing(*id) {
            return false;
        }
        match world.get::<AudioSource>(*e) {
            Some(source) if world.is_alive(*e) => {
                let pos = world.get::<GlobalTransform>(*e).map(|g| g.0.w_axis.truncate()).unwrap_or_default();
                let (gain, pan) = mix_params(&source, pos, l);
                m.set(*id, gain, pan, source.pitch, source.looping);
                true
            }
            _ => {
                m.stop(*id);
                false
            }
        }
    });
    drop(m);
    if let Some(mut state) = world.try_resource_mut::<AudioState>() {
        state.playing = audio.voices.keys().copied().collect();
    }
}

/// Stop every sound (play mode stopped). Previews keep playing.
pub fn reset(world: &mut World) {
    if let Some(mut audio) = world.try_resource_mut::<Audio>() {
        let ids: Vec<u64> = audio.voices.drain().map(|(_, id)| id).collect();
        let mut m = audio.mixer.lock().unwrap();
        for id in ids {
            m.stop(id);
        }
        m.take_finished();
        drop(m);
        audio.started = false;
    }
    if let Some(mut state) = world.try_resource_mut::<AudioState>() {
        state.playing.clear();
    }
    if let Some(mut c) = world.try_resource_mut::<AudioCommands>() {
        c.0.clear();
    }
}

/// Plays AudioSources and script sounds. `device: false` = silent / pulled by
/// the host (the web player).
pub struct AudioModule {
    pub device: bool,
}

impl Module for AudioModule {
    fn name(&self) -> &'static str {
        "audio"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["gamesys"]
    }
    fn build(&self, app: &mut App) {
        let audio = if self.device { Audio::with_device() } else { Audio::without_device() };
        app.world.insert_resource(audio);
        // After transforms propagate, so proximity uses this frame's positions.
        app.add_play_system(Stage::Last, "update_audio", update_audio);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kit_core::RunMode;
    use kit_scene::Transform;

    fn project() -> std::path::PathBuf {
        // A folder per call: tests run in parallel and must not delete each other's files.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("kit_audio_test_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sounds")).unwrap();
        let tone: Vec<i16> = (0..24_000).map(|i| ((i as f32 * 0.05).sin() * 10_000.0) as i16).collect();
        std::fs::write(dir.join("sounds/beep.wav"), wav_bytes(48_000, 1, &tone)).unwrap();
        dir
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_module(kit_resource::ResourceModule { root: project() })
            .add_module(kit_scene::SceneModule)
            .add_module(kit_gamesys::GameSysModule)
            .add_module(AudioModule { device: false });
        app
    }

    fn loudness(app: &App, frames: usize) -> (f32, f32) {
        let audio = app.world.resource::<Audio>();
        let mut out = vec![0.0; frames * 2];
        audio.mixer.lock().unwrap().mix(&mut out);
        out.chunks(2).fold((0.0, 0.0), |(l, r), f| (l + f[0].abs(), r + f[1].abs()))
    }

    #[test]
    fn sources_play_on_start_and_fade_with_distance() {
        let mut app = app();
        let cam = kit_scene::spawn_node(&mut app.world, "Camera", Transform::IDENTITY);
        app.world.insert(cam, kit_gamesys::Camera::default());
        let near = kit_scene::spawn_node(&mut app.world, "Near", Transform::from_xyz(2.0, 0.0, 0.0));
        let far = kit_scene::spawn_node(&mut app.world, "Far", Transform::from_xyz(-50.0, 0.0, 0.0));
        let spatial =
            AudioSource { clip: "sounds/beep.wav".into(), spatial: true, looping: true, ..Default::default() };
        app.world.insert(near, spatial.clone());
        app.world.insert(far, spatial);
        app.update(1.0 / 60.0);
        assert!(app.world.resource::<AudioState>().playing.contains(&near));
        // The near one is to the camera's right; the far one is beyond max distance.
        let (l, r) = loudness(&app, 2000);
        assert!(r > l * 3.0 && r > 10.0, "near source panned right: l={l} r={r}");

        // Scripts stop sounds through commands.
        app.world.resource_mut::<AudioCommands>().0.push(AudioCommand::Stop(near));
        app.update(1.0 / 60.0);
        let (l, r) = loudness(&app, 2000);
        assert!(l + r < 1e-3, "out of range and stopped: silence");
        assert!(!app.world.resource::<AudioState>().playing.contains(&near));

        // Stopping play mode silences everything and re-arms play-on-start.
        reset(&mut app.world);
        assert!(app.world.resource::<AudioState>().playing.is_empty());
        app.mode = RunMode::Edit;
        app.update(1.0 / 60.0);
        assert!(app.world.resource::<AudioState>().playing.is_empty(), "nothing plays in edit mode");
    }

    #[test]
    fn missing_files_report_to_the_console() {
        let mut app = app();
        let e = kit_scene::spawn_node(&mut app.world, "Bad", Transform::IDENTITY);
        app.world.insert(e, AudioSource { clip: "sounds/missing.mp3".into(), ..Default::default() });
        app.update(1.0 / 60.0);
        let console = app.world.resource::<kit_core::Console>();
        assert!(console.lines().iter().any(|l| l.text.contains("missing.mp3")), "{:?}", console.lines());
    }
}
