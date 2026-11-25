//! Switching scenes while a game runs: `Scene.Load("scenes/level2")` replaces
//! the current scene, `Scene.LoadAdditive` adds one on top, `Scene.Unload`
//! removes an additive one. Requests are queued during the frame and applied
//! at its end (Stage::Last), so nothing changes under a running script.

use anyhow::{Context, Result};
use kit_ecs::{Entity, World};

use crate::scene_file::{SCENE_EXTENSION, SceneFile, load_scene};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneOp {
    /// Replace every loaded scene with this one.
    Load(String),
    /// Load on top of what's there.
    LoadAdditive(String),
    /// Remove a scene loaded additively.
    Unload(String),
}

/// Queued scene changes (applied at the end of the frame).
#[derive(Default, Debug)]
pub struct SceneRequests(pub Vec<SceneOp>);

/// Which scene files are loaded and the entities each one spawned.
#[derive(Default, Debug)]
pub struct LoadedScenes {
    /// The main (non-additive) scene, first; additive ones after it.
    pub scenes: Vec<(String, Vec<Entity>)>,
}

impl LoadedScenes {
    pub fn current(&self) -> Option<&str> {
        self.scenes.first().map(|(p, _)| p.as_str())
    }
}

/// Something that happened to scenes this frame (for gameplay and Circuits
/// such as Multiplayer, which moves to the new scene's sector).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneEvent {
    Loaded { path: String, additive: bool },
    Unloaded { path: String },
    Failed { path: String, error: String },
}

#[derive(Default, Debug)]
pub struct SceneEvents(pub Vec<SceneEvent>);

/// `scenes/level2` → `scenes/level2.kscene`.
pub fn scene_file_path(path: &str) -> String {
    let p = path.trim().trim_start_matches("./").replace('\\', "/");
    if p.ends_with(&format!(".{SCENE_EXTENSION}")) { p } else { format!("{p}.{SCENE_EXTENSION}") }
}

pub fn request(world: &World, op: SceneOp) {
    if let Some(mut r) = world.try_resource_mut::<SceneRequests>() {
        r.0.push(op);
    }
}

/// Record the scene a game started with (its entities are what `Load` replaces).
pub fn mark_loaded(world: &mut World, path: &str, entities: Vec<Entity>) {
    if world.try_resource::<LoadedScenes>().is_none() {
        world.insert_resource(LoadedScenes::default());
    }
    world.resource_mut::<LoadedScenes>().scenes = vec![(scene_file_path(path), entities)];
}

fn despawn_all(world: &mut World, entities: &[Entity]) {
    for e in entities {
        // Children go with their parents; already-destroyed ones are skipped.
        if world.is_alive(*e) && !world.has::<kit_scene::Parent>(*e) {
            kit_scene::despawn_recursive(world, *e);
        }
    }
    for e in entities {
        if world.is_alive(*e) {
            kit_scene::despawn_recursive(world, *e);
        }
    }
}

fn read_scene(world: &World, path: &str) -> Result<SceneFile> {
    let res = world.try_resource::<kit_resource::Resources>().context("no project resources")?.clone();
    let text = res.read_text(path).with_context(|| format!("loading scene `{path}`"))?;
    SceneFile::from_json(&text).with_context(|| format!("in {path}"))
}

/// Apply queued scene changes (a play system at Stage::Last).
pub fn apply_scene_requests(world: &mut World) {
    let ops = match world.try_resource_mut::<SceneRequests>() {
        Some(mut r) if !r.0.is_empty() => std::mem::take(&mut r.0),
        _ => return,
    };
    if world.try_resource::<LoadedScenes>().is_none() {
        // Started without a record (e.g. the editor's Play): what's there now is the main scene.
        let mut all = Vec::new();
        for root in kit_scene::roots(world) {
            all.extend(kit_scene::descendants_and_self(world, root));
        }
        world.insert_resource(LoadedScenes { scenes: vec![(String::new(), all)] });
    }
    let mut events = Vec::new();
    for op in ops {
        match op {
            SceneOp::Load(path) => {
                let path = scene_file_path(&path);
                // Read first: a missing or broken file leaves the current scene running.
                match read_scene(world, &path) {
                    Err(e) => events.push(SceneEvent::Failed { path, error: format!("{e:#}") }),
                    Ok(scene) => {
                        let old = std::mem::take(&mut world.resource_mut::<LoadedScenes>().scenes);
                        for (_, entities) in &old {
                            despawn_all(world, entities);
                        }
                        for (p, _) in old.iter().filter(|(p, _)| !p.is_empty()) {
                            events.push(SceneEvent::Unloaded { path: p.clone() });
                        }
                        match load_scene(world, &scene) {
                            Ok(spawned) => {
                                world.resource_mut::<LoadedScenes>().scenes = vec![(path.clone(), spawned)];
                                events.push(SceneEvent::Loaded { path, additive: false });
                            }
                            Err(e) => events.push(SceneEvent::Failed { path, error: format!("{e:#}") }),
                        }
                    }
                }
            }
            SceneOp::LoadAdditive(path) => {
                let path = scene_file_path(&path);
                if world.resource::<LoadedScenes>().scenes.iter().any(|(p, _)| *p == path) {
                    continue; // already loaded
                }
                match read_scene(world, &path).and_then(|s| load_scene(world, &s)) {
                    Ok(spawned) => {
                        world.resource_mut::<LoadedScenes>().scenes.push((path.clone(), spawned));
                        events.push(SceneEvent::Loaded { path, additive: true });
                    }
                    Err(e) => events.push(SceneEvent::Failed { path, error: format!("{e:#}") }),
                }
            }
            SceneOp::Unload(path) => {
                let path = scene_file_path(&path);
                let found = {
                    let mut loaded = world.resource_mut::<LoadedScenes>();
                    // The main scene stays; replace it with Load instead.
                    let i = loaded.scenes.iter().skip(1).position(|(p, _)| *p == path).map(|i| i + 1);
                    i.map(|i| loaded.scenes.remove(i))
                };
                if let Some((_, entities)) = found {
                    despawn_all(world, &entities);
                    events.push(SceneEvent::Unloaded { path });
                }
            }
        }
    }
    for e in &events {
        if let SceneEvent::Failed { path, error } = e {
            log::error!("scene {path}: {error}");
        }
    }
    world.resource_mut::<SceneEvents>().0.extend(events);
}

/// Clear last frame's scene events (Stage::First).
pub fn clear_scene_events(world: &mut World) {
    if let Some(mut e) = world.try_resource_mut::<SceneEvents>() {
        e.0.clear();
    }
}
