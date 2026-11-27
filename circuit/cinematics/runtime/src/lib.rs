//! The Cinematics Circuit inside a running Kit game.
//!
//! Dormant unless the project has `kit.cinematics` installed and enabled
//! (circuits.lock). Then:
//!
//! - an object with a **Cinematic** component is a director: the cameras
//!   inside it with a **Cinematic Shot** component are its shots, played in
//!   Hierarchy order;
//! - a shot shows the game through its camera for its duration, cutting or
//!   blending from the previous one. A shot camera with a path sketched in
//!   Canvas mode flies along it; it can also keep looking at an object, zoom,
//!   shake and show a caption;
//! - while it plays: letterbox bars, fades in and out, and a skip key;
//!   afterwards the game's own camera comes back;
//! - it starts on its own (Play on start), from scripts
//!   (`Cinematic.Play("Intro")`), or from diagrams (a custom event named
//!   `PlayCinematic` with the name as its text). Diagrams hear
//!   `CinematicStarted` and `CinematicFinished` (with the name).

use glam::{Quat, Vec3};
use kit_core::{App, Console, Module, Stage, Time};
use kit_ecs::{Entity, World};
use kit_gamesys::{
    BridgeValue, Camera, CinematicControl, CinematicOverlay, CinematicRequest, ExtraData, GraphEvent, GraphEvents,
    PathFollow, Projection,
};
use kit_scene::{GlobalTransform, Name, Parent, Transform};
use serde::Deserialize;

pub const CIRCUIT_ID: &str = "kit.cinematics";
/// Component ids (circuit.toml), stored on objects as extra scene data.
pub const DIRECTOR: &str = "cinematic";
pub const SHOT: &str = "cinematic_shot";

/// Seconds the letterbox bars take to slide in or out.
const BARS: f32 = 0.45;

pub struct CinematicsModule;

impl Module for CinematicsModule {
    fn name(&self) -> &'static str {
        "cinematics"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["gamesys"]
    }
    fn build(&self, app: &mut App) {
        app.world.insert_resource(Cinematics::default());
        // After scripts' Update and LateUpdate, so the shot camera wins.
        app.add_play_system(Stage::PostUpdate, "cinematics", update);
        // Before paths move: undo this shot's blend / shake / look-at, so a
        // sketched move carries on from its own pose.
        app.add_play_system(Stage::PreUpdate, "cinematics_restore", restore_pose);
        // Every mode: forget everything when play stops.
        app.add_system(Stage::Last, "cinematics_stop", stop_when_not_playing);
    }
}

/// Settings of a Cinematic (director) component.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Director {
    pub name: String,
    pub play_on_start: bool,
    pub letterbox: bool,
    pub skippable: bool,
    pub fade_in: f32,
    pub fade_out: f32,
    #[serde(rename = "loop")]
    pub looping: bool,
}

impl Default for Director {
    fn default() -> Self {
        Self { name: String::new(), play_on_start: false, letterbox: true, skippable: true, fade_in: 0.6, fade_out: 0.6, looping: false }
    }
}

/// Settings of a Cinematic Shot component.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ShotSettings {
    pub duration: f32,
    pub transition: String,
    pub blend: f32,
    pub look_at: String,
    pub zoom_to: f32,
    pub caption: String,
    pub shake: f32,
}

impl Default for ShotSettings {
    fn default() -> Self {
        Self { duration: 3.0, transition: "cut".into(), blend: 1.0, look_at: String::new(), zoom_to: 0.0, caption: String::new(), shake: 0.0 }
    }
}

fn read<T: for<'de> Deserialize<'de>>(world: &World, e: Entity, key: &str) -> Option<T> {
    world.get::<ExtraData>(e).and_then(|x| x.0.get(key).cloned()).and_then(|v| serde_json::from_value(v).ok())
}

/// One shot while its cinematic plays.
struct Shot {
    camera: Entity,
    settings: ShotSettings,
    /// The camera as authored, put back afterwards.
    home: Transform,
    fov: Option<f32>,
    path: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Playing,
    /// Faded to black at the end; the game's camera is back, fading in.
    Returning,
}

struct Playback {
    name: String,
    director: Entity,
    settings: Director,
    shots: Vec<Shot>,
    index: usize,
    /// Seconds into the current shot / the whole cinematic.
    shot_time: f32,
    time: f32,
    phase: Phase,
    /// Seconds into the return fade.
    returning: f32,
    /// Where the previous view was when this shot started (for blends).
    blend_from: Option<(Vec3, Quat)>,
    /// Cameras' `active` switches before the cinematic took over.
    cameras: Vec<(Entity, bool)>,
    /// The shot camera's own pose this frame (before blends and shakes).
    raw: Option<(Entity, Transform)>,
    /// What was on screen last frame (after look-at, blends and shakes):
    /// the next shot blends from here.
    shown: Option<(Vec3, Quat)>,
    /// Field of view on screen last frame, and where the current blend's zoom starts.
    shown_fov: Option<f32>,
    blend_fov: Option<f32>,
}

/// The Circuit's state (a world resource).
#[derive(Default)]
pub struct Cinematics {
    /// Checked the lockfile this play session; `Some(true)` = installed and enabled.
    enabled: Option<bool>,
    started: bool,
    playing: Option<Playback>,
    played: bool,
}

/// Is Cinematics installed and enabled in this project's circuits.lock?
pub fn enabled_in_lock(text: &str) -> bool {
    let Ok(v) = text.parse::<toml::Table>() else { return false };
    v.get("circuit").and_then(|c| c.as_array()).is_some_and(|list| {
        list.iter().any(|c| {
            c.get("id").and_then(|v| v.as_str()) == Some(CIRCUIT_ID) && c.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)
        })
    })
}

fn warn(world: &World, text: impl Into<String>) {
    if let Some(mut c) = world.try_resource_mut::<Console>() {
        c.warn("cinematics", text.into());
    }
}

/// Directors in the scene (entity, settings), named by their setting or the object's name.
fn directors(world: &World) -> Vec<(Entity, Director)> {
    let mut out = Vec::new();
    for e in world.query::<(ExtraData,)>() {
        if let Some(mut d) = read::<Director>(world, e, DIRECTOR) {
            if d.name.trim().is_empty() {
                d.name = world.get::<Name>(e).map(|n| n.0.clone()).unwrap_or_default();
            }
            out.push((e, d));
        }
    }
    out
}

/// Shot cameras inside a director, in Hierarchy order.
fn shots_of(world: &World, director: Entity) -> Vec<Entity> {
    kit_scene::descendants_and_self(world, director)
        .into_iter()
        .filter(|&e| e != director && world.has::<Camera>(e) && read::<ShotSettings>(world, e, SHOT).is_some())
        .collect()
}

fn world_pose(world: &World, e: Entity) -> (Vec3, Quat) {
    let m = match world.get::<Transform>(e) {
        Some(t) => parent_matrix(world, e) * t.matrix(),
        None => world.get::<GlobalTransform>(e).map(|g| g.0).unwrap_or_default(),
    };
    let (_, r, t) = m.to_scale_rotation_translation();
    (t, r.normalize())
}

fn parent_matrix(world: &World, e: Entity) -> glam::Mat4 {
    world.get::<Parent>(e).and_then(|p| world.get::<GlobalTransform>(p.0)).map(|g| g.0).unwrap_or(glam::Mat4::IDENTITY)
}

/// Put `e` at a world pose (converted into its parent's space).
fn set_world_pose(world: &World, e: Entity, pos: Vec3, rot: Quat) {
    let parent = parent_matrix(world, e);
    let (_, prot, _) = parent.to_scale_rotation_translation();
    if let Some(mut t) = world.get_mut::<Transform>(e) {
        t.translation = parent.inverse().transform_point3(pos);
        t.rotation = (prot.inverse() * rot).normalize();
    }
}

fn look_rotation(dir: Vec3) -> Option<Quat> {
    let f = dir.normalize_or_zero();
    if f == Vec3::ZERO {
        return None;
    }
    let right = f.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
    let up = right.cross(f);
    Some(Quat::from_mat3(&glam::Mat3::from_cols(right, up, -f)).normalize())
}

fn fov_of(world: &World, e: Entity) -> Option<f32> {
    match world.get::<Camera>(e)?.projection {
        Projection::Perspective { fov_y, .. } => Some(fov_y),
        Projection::Orthographic { .. } => None,
    }
}

fn set_fov(world: &World, e: Entity, value: f32) {
    if let Some(mut c) = world.get_mut::<Camera>(e) {
        if let Projection::Perspective { fov_y, .. } = &mut c.projection {
            *fov_y = value.clamp(1.0, 179.0);
        }
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn emit(world: &World, event: &str, name: &str) {
    if let Some(mut q) = world.try_resource_mut::<GraphEvents>() {
        q.0.push(GraphEvent { name: event.into(), payload: Some(BridgeValue::Text(name.into())), sender: None });
    }
}

/// Only the chosen camera shows the game.
fn show_through(world: &World, camera: Entity) {
    for c in world.query::<(Camera,)>() {
        if let Some(mut cam) = world.get_mut::<Camera>(c) {
            cam.active = c == camera;
        }
    }
}

/// Paths on shot cameras only run during their shot.
fn hold_paths(world: &mut World) {
    for (d, _) in directors(world) {
        for cam in shots_of(world, d) {
            if let Some(mut p) = world.get_mut::<PathFollow>(cam) {
                p.playing = false;
            }
        }
    }
}

fn start(world: &mut World, cin: &mut Cinematics, name: &str) -> Result<(), String> {
    if cin.playing.is_some() {
        finish(world, cin, false);
    }
    let Some((director, settings)) = directors(world).into_iter().find(|(_, d)| d.name.eq_ignore_ascii_case(name)) else {
        return Err(format!("no cinematic named \"{name}\" in this scene"));
    };
    let shots: Vec<Shot> = shots_of(world, director)
        .into_iter()
        .map(|camera| Shot {
            settings: read(world, camera, SHOT).unwrap_or_default(),
            home: world.get::<Transform>(camera).map(|t| *t).unwrap_or_default(),
            fov: fov_of(world, camera),
            path: world.get::<PathFollow>(camera).is_some_and(|p| p.points.len() >= 2),
            camera,
        })
        .collect();
    if shots.is_empty() {
        return Err(format!("the cinematic \"{name}\" has no shots: put cameras with a Cinematic Shot inside it"));
    }
    let cameras = world.query::<(Camera,)>().into_iter().map(|c| (c, world.get::<Camera>(c).is_some_and(|c| c.active))).collect();
    let game_cam = kit_gamesys::active_camera(world);
    let from = game_cam.map(|c| world_pose(world, c));
    let from_fov = game_cam.and_then(|c| fov_of(world, c));
    cin.playing = Some(Playback {
        name: settings.name.clone(),
        director,
        settings,
        shots,
        index: 0,
        shot_time: 0.0,
        time: 0.0,
        phase: Phase::Playing,
        returning: 0.0,
        blend_from: from,
        cameras,
        raw: None,
        shown: None,
        shown_fov: from_fov,
        blend_fov: from_fov,
    });
    begin_shot(world, cin.playing.as_mut().unwrap(), 0);
    emit(world, "CinematicStarted", name);
    Ok(())
}

fn begin_shot(world: &mut World, pb: &mut Playback, index: usize) {
    // Blend from exactly what was on screen (the camera's own transform may
    // lack its look-at turn, blend or shake).
    let previous = pb.shown.or_else(|| pb.shots.get(pb.index).map(|s| world_pose(world, s.camera)));
    if index > 0 {
        pb.blend_from = previous;
        pb.blend_fov = pb.shown_fov;
    }
    pb.index = index;
    pb.shot_time = 0.0;
    let shot = &pb.shots[index];
    if let Some(mut t) = world.get_mut::<Transform>(shot.camera) {
        *t = shot.home;
    }
    if shot.path {
        // Start its sketched move from the beginning.
        world.remove::<kit_path::PathState>(shot.camera);
        if let Some(mut p) = world.get_mut::<PathFollow>(shot.camera) {
            p.playing = true;
        }
    }
    show_through(world, shot.camera);
}

/// Put everything back. `fade_back`: fade in from black over the director's fade-out time.
fn finish(world: &mut World, cin: &mut Cinematics, fade_back: bool) {
    let Some(mut pb) = cin.playing.take() else { return };
    pb.raw = None;
    for shot in &pb.shots {
        if !world.is_alive(shot.camera) {
            continue;
        }
        if let Some(mut t) = world.get_mut::<Transform>(shot.camera) {
            *t = shot.home;
        }
        if let Some(f) = shot.fov {
            set_fov(world, shot.camera, f);
        }
        if let Some(mut p) = world.get_mut::<PathFollow>(shot.camera) {
            p.playing = false;
        }
        world.remove::<kit_path::PathState>(shot.camera);
    }
    for (c, active) in &pb.cameras {
        if let Some(mut cam) = world.get_mut::<Camera>(*c) {
            cam.active = *active;
        }
    }
    emit(world, "CinematicFinished", &pb.name);
    if fade_back && pb.settings.fade_out > 0.0 {
        pb.phase = Phase::Returning;
        pb.returning = 0.0;
        cin.playing = Some(pb);
    } else if let Some(mut o) = world.try_resource_mut::<CinematicOverlay>() {
        *o = CinematicOverlay::default();
    }
}

/// First frame of play: is the Circuit on? If so, hold shot cameras' paths
/// (before the path module moves anything).
fn check_enabled(world: &mut World, cin: &mut Cinematics) {
    if cin.enabled.is_some() {
        return;
    }
    let on = world
        .try_resource::<kit_resource::Resources>()
        .and_then(|r| r.read_text("circuits.lock").ok())
        .is_some_and(|t| enabled_in_lock(&t));
    cin.enabled = Some(on);
    if let Some(mut c) = world.try_resource_mut::<CinematicControl>() {
        c.available = on;
    }
    if on {
        hold_paths(world);
    }
}

fn update(world: &mut World) {
    let Some(mut cin) = world.remove_resource::<Cinematics>() else { return };
    cin.played = true;
    check_enabled(world, &mut cin);
    if cin.enabled == Some(true) {
        step(world, &mut cin);
    } else if let Some(mut c) = world.try_resource_mut::<CinematicControl>() {
        c.requests.clear();
        c.playing = None;
    }
    world.insert_resource(cin);
}

fn step(world: &mut World, cin: &mut Cinematics) {
    let dt = world.resource::<Time>().delta;
    if !cin.started {
        cin.started = true;
        if let Some((_, d)) = directors(world).into_iter().find(|(_, d)| d.play_on_start) {
            if let Err(e) = start(world, cin, &d.name) {
                warn(world, e);
            }
        }
    }

    // Requests from scripts, and from diagrams' custom events (taken out of
    // the queue: they're addressed to us).
    let mut requests = world.try_resource_mut::<CinematicControl>().map(|mut c| std::mem::take(&mut c.requests)).unwrap_or_default();
    if let Some(mut q) = world.try_resource_mut::<GraphEvents>() {
        q.0.retain(|ev| match (ev.name.as_str(), &ev.payload) {
            ("PlayCinematic", Some(BridgeValue::Text(n))) => {
                requests.push(CinematicRequest::Play(n.clone()));
                false
            }
            ("StopCinematic", _) => {
                requests.push(CinematicRequest::Stop);
                false
            }
            _ => true,
        });
    }
    for r in requests {
        match r {
            CinematicRequest::Play(name) => {
                if let Err(e) = start(world, cin, &name) {
                    warn(world, e);
                }
            }
            CinematicRequest::Stop => finish(world, cin, false),
            CinematicRequest::Skip => {
                if cin.playing.as_ref().is_some_and(|p| p.phase == Phase::Playing) {
                    finish(world, cin, true);
                }
            }
        }
    }

    let mut overlay = CinematicOverlay::default();
    let mut done = false;
    if let Some(pb) = cin.playing.as_mut() {
        if !world.is_alive(pb.director) {
            done = true;
        } else if pb.phase == Phase::Returning {
            pb.returning += dt;
            let k = (pb.returning / pb.settings.fade_out.max(1e-3)).min(1.0);
            overlay.fade = 1.0 - smooth(k);
            if pb.settings.letterbox {
                overlay.letterbox = 1.0 - smooth(pb.returning / BARS);
            }
            if k >= 1.0 {
                cin.playing = None;
            }
        } else {
            pb.time += dt;
            pb.shot_time += dt;
            // Skip key.
            let skip = pb.settings.skippable
                && world.try_resource::<kit_input::Input>().is_some_and(|i| {
                    ["Escape", "Space", "Enter", "PadStart", "PadA"].iter().any(|k| i.pressed(k))
                });
            let total: f32 = pb.shots.iter().map(|s| s.settings.duration.max(0.05)).sum();
            if skip {
                done = true;
            } else {
                if pb.shot_time >= pb.shots[pb.index].settings.duration.max(0.05) {
                    if pb.index + 1 < pb.shots.len() {
                        let next = pb.index + 1;
                        begin_shot(world, pb, next);
                    } else if pb.settings.looping {
                        pb.time = 0.0;
                        begin_shot(world, pb, 0);
                    } else {
                        done = true;
                    }
                }
                if !done {
                    frame(world, pb);
                    overlay.caption = pb.shots[pb.index].settings.caption.clone();
                    if pb.settings.letterbox {
                        overlay.letterbox = smooth(pb.time / BARS);
                    }
                    if pb.settings.fade_in > 0.0 && pb.time < pb.settings.fade_in {
                        overlay.fade = 1.0 - smooth(pb.time / pb.settings.fade_in);
                    }
                    // Fade to black at the very end, so the game's camera comes back hidden.
                    let left = total - pb.time;
                    if !pb.settings.looping && pb.settings.fade_out > 0.0 && left < pb.settings.fade_out {
                        overlay.fade = overlay.fade.max(smooth(1.0 - left / pb.settings.fade_out));
                    }
                }
            }
        }
    }
    if done {
        let back = cin.playing.as_ref().is_some_and(|p| world.is_alive(p.director));
        finish(world, cin, back);
        if let Some(pb) = cin.playing.as_ref() {
            overlay.fade = 1.0;
            overlay.letterbox = if pb.settings.letterbox { 1.0 } else { 0.0 };
        }
    }
    if let Some(mut o) = world.try_resource_mut::<CinematicOverlay>() {
        *o = overlay;
    }
    if let Some(mut c) = world.try_resource_mut::<CinematicControl>() {
        match cin.playing.as_ref().filter(|p| p.phase == Phase::Playing) {
            Some(p) => {
                c.playing = Some(p.name.clone());
                c.shot = p.index;
                c.time = p.time;
            }
            None => {
                c.playing = None;
                c.shot = 0;
                c.time = 0.0;
            }
        }
    }
    kit_scene::propagate_transforms(world);
}

/// Pose the current shot's camera for this frame.
fn frame(world: &mut World, pb: &mut Playback) {
    let shot = &pb.shots[pb.index];
    let cam = shot.camera;
    let s = &shot.settings;
    // Without a path the camera holds its authored place (blends and shakes
    // write over it each frame); a path moves it, and we start from there.
    if !shot.path {
        if let Some(mut t) = world.get_mut::<Transform>(cam) {
            *t = shot.home;
        }
    }
    pb.raw = world.get::<Transform>(cam).map(|t| (cam, *t));
    let (mut pos, mut rot) = world_pose(world, cam);
    if !s.look_at.trim().is_empty() {
        if let Some(target) = kit_scene::find_by_name(world, s.look_at.trim()) {
            let (tp, _) = world_pose(world, target);
            if let Some(r) = look_rotation(tp - pos) {
                rot = r;
            }
        }
    }
    let blending = s.transition.eq_ignore_ascii_case("blend") && s.blend > 0.0 && pb.shot_time < s.blend;
    if blending {
        if let Some((fp, fr)) = pb.blend_from {
            let k = smooth(pb.shot_time / s.blend);
            pos = fp.lerp(pos, k);
            rot = fr.slerp(rot, k).normalize();
        }
    }
    if s.shake > 0.0 {
        let t = pb.time * 23.0;
        let wobble = Vec3::new((t * 1.3).sin() + (t * 2.9).sin() * 0.5, (t * 1.7).cos() + (t * 3.1).sin() * 0.5, 0.0) * 0.05 * s.shake;
        pos += rot * wobble;
    }
    set_world_pose(world, cam, pos, rot);
    pb.shown = Some((pos, rot));
    // Zoom: this shot's own (with its Zoom to), blended in from what was on
    // screen like position and direction, so nothing pops at a transition.
    if let Some(own) = shot.fov {
        let mut fov = if s.zoom_to > 0.0 {
            let k = smooth(pb.shot_time / s.duration.max(0.05));
            own + (s.zoom_to - own) * k
        } else {
            own
        };
        if blending {
            if let Some(from) = pb.blend_fov {
                fov = from + (fov - from) * smooth(pb.shot_time / s.blend);
            }
        }
        set_fov(world, cam, fov);
        pb.shown_fov = Some(fov);
    }
}

fn restore_pose(world: &mut World) {
    if let Some(mut cin) = world.remove_resource::<Cinematics>() {
        check_enabled(world, &mut cin);
        world.insert_resource(cin);
    }
    let raw = world.try_resource::<Cinematics>().and_then(|c| c.playing.as_ref().and_then(|p| p.raw));
    if let Some((cam, t)) = raw {
        if let Some(mut now) = world.get_mut::<Transform>(cam) {
            *now = t;
        }
    }
}

/// Play systems stop running when play stops; then put everything back.
fn stop_when_not_playing(world: &mut World) {
    let Some(mut cin) = world.try_resource_mut::<Cinematics>() else { return };
    if std::mem::take(&mut cin.played) {
        return;
    }
    if cin.enabled.is_some() {
        *cin = Cinematics::default();
        drop(cin);
        if let Some(mut o) = world.try_resource_mut::<CinematicOverlay>() {
            *o = CinematicOverlay::default();
        }
        if let Some(mut c) = world.try_resource_mut::<CinematicControl>() {
            *c = CinematicControl::default();
        }
    }
}

#[cfg(test)]
mod tests;
