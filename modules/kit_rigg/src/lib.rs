//! # kit_rigg
//!
//! **Rigg**: Kit's volume rigs. Spheres, capsules and boxes placed inside a
//! model are its controls: move, rotate or scale one and the mesh around it
//! follows smoothly. No bones.
//!
//! * Data: [`kit_gamesys::Rig`] on the model, [`kit_gamesys::RigVolume`] on
//!   descendant objects (so selection, gizmos, undo, save/load and export all
//!   work like any other object).
//! * [`bind`]: smooth, normalized weights from the volumes' shapes in the
//!   rest pose. [`deform`]: linear or dual-quaternion ("Preserve Volume")
//!   blending, with normals carried along.
//! * [`clip`]: keyed volume poses over time; clips play in the game
//!   (`Rig::autoplay`, or [`play`]).
//! * [`preset`]: a humanoid preset fitted to the model.
//!
//! The deformed mesh goes into [`kit_render::draw::Deformed`], which every
//! renderer (desktop and web) draws in place of the model's mesh.

pub mod bind;
pub mod clip;
pub mod deform;
pub mod preset;

use bind::{BindVolume, Influences};
use deform::Motion;
use glam::{Mat4, Vec3};
use kit_core::{App, Module, RunMode, Stage};
use kit_ecs::{Entity, World};
use kit_gamesys::{Model, Rig, RigVolume};
use kit_render::Vertex;
use kit_render::draw::{Deformed, DeformedMesh, ImportedGeometry, MeshPart};
use kit_resource::Resources;
use kit_scene::{GlobalTransform, Name, Parent, Transform};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// What the editor wants colored (influence preview).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RigPreview {
    /// The rig being previewed.
    pub rig: Option<Entity>,
    /// Show influence colors.
    pub weights: bool,
    /// Color by this volume's weight only (else every volume in its own color).
    pub focus: Option<Entity>,
    /// Let the editor change link lengths while fitting volumes.
    pub fitting: bool,
}

/// A rest mesh: triangle-soup vertices and per-material ranges.
#[derive(Clone)]
struct RestMesh {
    vertices: Vec<Vertex>,
    parts: Vec<MeshPart>,
    min: Vec3,
    max: Vec3,
}

struct Binding {
    key: u64,
    mesh: String,
    volumes: Vec<Entity>,
    weights: Vec<Influences>,
    pose_key: u64,
}

/// Rig state (a world resource): bindings, cached rest meshes, playback.
#[derive(Default)]
pub struct RigRuntime {
    meshes: HashMap<String, Option<RestMesh>>,
    bindings: HashMap<Entity, Binding>,
    revision: u64,
    /// Playing clips: rig → (clip name, play time when it started).
    playing: HashMap<Entity, (String, f64)>,
    started: bool,
}

impl RigRuntime {
    /// Bounds of a model's rest mesh (model space), once it has loaded.
    pub fn rest_bounds(&self, mesh: &str) -> Option<(Vec3, Vec3)> {
        self.meshes.get(mesh)?.as_ref().map(|m| (m.min, m.max))
    }

    /// Influences per rest vertex (for tests and tools).
    pub fn weights(&self, rig: Entity) -> Option<&[Influences]> {
        self.bindings.get(&rig).map(|b| b.weights.as_slice())
    }

    /// The volumes in binding order.
    pub fn volumes(&self, rig: Entity) -> Option<&[Entity]> {
        self.bindings.get(&rig).map(|b| b.volumes.as_slice())
    }
}

fn mesh_key(model: &Model) -> String {
    if model.mesh_asset.is_empty() { format!("builtin:{:?}", model.mesh) } else { model.mesh_asset.clone() }
}

/// The model's rest mesh as triangle soup (imported: the "rest" frame if the
/// importer saved one, else frame 0; built-in meshes: as is).
fn load_rest(model: &Model, res: Option<&Resources>) -> Option<RestMesh> {
    let (vertices, parts) = if model.mesh_asset.is_empty() {
        let data = kit_render::build_mesh(model.mesh);
        let vertices: Vec<Vertex> = data.indices.iter().map(|i| data.vertices[*i as usize]).collect();
        let parts = vec![MeshPart { material: 0, first: 0, count: vertices.len() as u32 }];
        (vertices, parts)
    } else {
        let geometry = ImportedGeometry::load(res?, &model.mesh_asset).map_err(|e| log::warn!("rig: {e:#}")).ok()?;
        let frame =
            geometry.asset.as_ref().and_then(|a| a.rest).unwrap_or(0).min(geometry.frames.len().saturating_sub(1));
        let mut vertices = Vec::new();
        let mut parts = Vec::new();
        for part in geometry.frames.get(frame)? {
            let first = vertices.len() as u32;
            for i in &geometry.data.indices[part.first as usize..(part.first + part.count) as usize] {
                vertices.push(geometry.data.vertices[*i as usize]);
            }
            parts.push(MeshPart { material: part.material, first, count: vertices.len() as u32 - first });
        }
        (vertices, parts)
    };
    if vertices.is_empty() {
        return None;
    }
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for v in &vertices {
        min = min.min(Vec3::from_array(v.pos));
        max = max.max(Vec3::from_array(v.pos));
    }
    Some(RestMesh { vertices, parts, min, max })
}

/// The rig's volumes (descendants of the rig object with a [`RigVolume`]).
pub fn volumes_of(world: &World, rig: Entity) -> Vec<Entity> {
    kit_scene::descendants_and_self(world, rig)
        .into_iter()
        .filter(|e| *e != rig && world.has::<RigVolume>(*e))
        .collect()
}

/// A volume's transform in rig space (relative to the rig object).
pub fn rig_space(world: &World, rig: Entity, volume: Entity) -> Mat4 {
    let r = world.get::<GlobalTransform>(rig).map_or(Mat4::IDENTITY, |g| g.0);
    let v = world.get::<GlobalTransform>(volume).map_or(Mat4::IDENTITY, |g| g.0);
    r.inverse() * v
}

/// A rigid joint to the parent volume, measured in the model's space.
fn parent_joint(world: &World, volume: Entity) -> Option<(Entity, Vec3)> {
    let parent = world.get::<Parent>(volume)?.0;
    let rest = world.get::<RigVolume>(volume)?.rest?;
    let parent_rest = world.get::<RigVolume>(parent)?.rest?;
    let offset = rest.w_axis.truncate() - parent_rest.w_axis.truncate();
    offset.is_finite().then_some((parent, offset))
}

fn at_fitted_length(offset: Vec3, rest: Vec3) -> Vec3 {
    let length = rest.length();
    if (offset.length() - length).abs() <= length * f32::EPSILON {
        return offset;
    }
    offset.try_normalize().map_or(rest, |direction| direction * length)
}

/// Move a rig control as a kinematic handle. Each rigid joint transfers the
/// movement back up the chain. Branches follow their parents in the hierarchy.
pub fn move_control(world: &mut World, rig: Entity, control: Entity, position: Vec3) {
    kit_scene::propagate_transforms(world);
    let Some(model) = world.get::<GlobalTransform>(rig).map(|g| g.0) else { return };
    let inverse = model.inverse();
    let desired = inverse.transform_point3(position);
    if !desired.is_finite() {
        return;
    }

    let mut chain = vec![(control, desired)];
    let mut node = control;
    let mut center = desired;
    while let Some((parent, rest)) = parent_joint(world, node) {
        let parent_center = rig_space(world, rig, parent).w_axis.truncate();
        let offset = at_fitted_length(center - parent_center, rest);
        center -= offset;
        chain.push((parent, center));
        node = parent;
    }

    let mut moved_parent = None;
    for (node, center) in chain.into_iter().rev() {
        let position = model.transform_point3(center);
        let parent =
            world.get::<Parent>(node).and_then(|p| world.get::<GlobalTransform>(p.0)).map_or(Mat4::IDENTITY, |g| g.0);
        let origin = moved_parent.unwrap_or_else(|| parent.w_axis.truncate());
        let translation = parent.inverse().transform_vector3(position - origin);
        if translation.is_finite() {
            if let Some(mut t) = world.get_mut::<Transform>(node) {
                t.translation = translation;
            }
        }
        moved_parent = Some(position);
    }
    kit_scene::propagate_transforms(world);
}

fn constrain_translation(world: &World, rig: Entity, volume: Entity, requested: Vec3) -> Vec3 {
    let Some((parent, rest)) = parent_joint(world, volume) else { return requested };
    let parent_in_rig = rig_space(world, rig, parent);
    let offset = parent_in_rig.transform_vector3(requested);
    let constrained = at_fitted_length(offset, rest);
    if offset == constrained {
        return requested;
    }
    let local = parent_in_rig.inverse().transform_vector3(constrained);
    if local.is_finite() { local } else { requested }
}

/// Make the current volume placement the rest pose (the mesh goes back to
/// undeformed, and weights are recomputed). Used while fitting volumes.
pub fn rebind(world: &mut World, rig: Entity) {
    kit_scene::propagate_transforms(world);
    for v in volumes_of(world, rig) {
        let m = rig_space(world, rig, v);
        if let Some(mut vol) = world.get_mut::<RigVolume>(v) {
            vol.rest = Some(m);
        }
    }
}

/// Put every volume back where it was at rest.
pub fn reset_pose(world: &mut World, rig: Entity) {
    kit_scene::propagate_transforms(world);
    let rig_m = world.get::<GlobalTransform>(rig).map_or(Mat4::IDENTITY, |g| g.0);
    // Parents first, so children's locals are computed against rested parents.
    for v in volumes_of(world, rig) {
        let Some(rest) = world.get::<RigVolume>(v).and_then(|r| r.rest) else { continue };
        kit_scene::set_world_matrix(world, v, rig_m * rest);
        kit_scene::propagate_transforms(world);
    }
}

/// Mirror a volume across the rig's X axis onto its L_/R_ counterpart
/// (shape and pose). Returns the counterpart, if one exists.
pub fn mirror(world: &mut World, rig: Entity, volume: Entity) -> Option<Entity> {
    let name = world.get::<Name>(volume)?.0.clone();
    let other_name = if let Some(rest) = name.strip_prefix("L_") {
        format!("R_{rest}")
    } else if let Some(rest) = name.strip_prefix("R_") {
        format!("L_{rest}")
    } else {
        return None;
    };
    let other =
        volumes_of(world, rig).into_iter().find(|e| world.get::<Name>(*e).is_some_and(|n| n.0 == other_name))?;
    kit_scene::propagate_transforms(world);
    let flip = Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0));
    let mirrored = flip * rig_space(world, rig, volume) * flip;
    // Mirroring a rigid transform twice by X keeps it rigid.
    let rig_m = world.get::<GlobalTransform>(rig).map_or(Mat4::IDENTITY, |g| g.0);
    kit_scene::set_world_matrix(world, other, rig_m * mirrored);
    let src = world.get::<RigVolume>(volume)?.clone();
    if let Some(mut dst) = world.get_mut::<RigVolume>(other) {
        dst.shape = src.shape;
        dst.influence = src.influence;
        dst.falloff = src.falloff;
    }
    Some(other)
}

/// Save the current pose by name (replacing one with the same name).
pub fn save_pose(world: &mut World, rig: Entity, name: &str) {
    let volumes: Vec<(String, [f32; 10])> = volumes_of(world, rig)
        .into_iter()
        .filter_map(|v| {
            Some((world.get::<Name>(v)?.0.clone(), kit_gamesys::rig::pack_transform(&*world.get::<Transform>(v)?)))
        })
        .collect();
    if let Some(mut r) = world.get_mut::<Rig>(rig) {
        r.poses.retain(|p| p.name != name);
        r.poses.push(kit_gamesys::RigPose { name: name.to_owned(), volumes });
    }
}

/// Apply a saved pose (volumes it doesn't mention stay put).
pub fn apply_pose(world: &mut World, rig: Entity, name: &str) -> bool {
    let Some(pose) = world.get::<Rig>(rig).and_then(|r| r.poses.iter().find(|p| p.name == name).cloned()) else {
        return false;
    };
    let by_name: HashMap<String, Entity> =
        volumes_of(world, rig).into_iter().filter_map(|v| Some((world.get::<Name>(v)?.0.clone(), v))).collect();
    for (n, t) in &pose.volumes {
        if let Some(e) = by_name.get(n) {
            if let Some(mut tr) = world.get_mut::<Transform>(*e) {
                *tr = kit_gamesys::rig::unpack_transform(t);
            }
        }
    }
    true
}

/// Pose the volumes from a clip at `time` (the timeline and playback).
pub fn pose_clip(world: &mut World, rig: Entity, clip_name: &str, time: f32) -> bool {
    let Some(clip) = world.get::<Rig>(rig).and_then(|r| r.clips.iter().find(|c| c.name == clip_name).cloned()) else {
        return false;
    };
    let by_name: HashMap<String, Entity> =
        volumes_of(world, rig).into_iter().filter_map(|v| Some((world.get::<Name>(v)?.0.clone(), v))).collect();
    for track in &clip.tracks {
        let (Some(e), Some(t)) = (by_name.get(&track.volume), clip::sample(track, time)) else { continue };
        if let Some(mut tr) = world.get_mut::<Transform>(*e) {
            *tr = t;
        }
    }
    true
}

/// Key every volume's current pose into a clip at `time`.
pub fn key_pose(world: &mut World, rig: Entity, clip_name: &str, time: f32) {
    let poses: Vec<(String, Transform)> = volumes_of(world, rig)
        .into_iter()
        .filter_map(|v| Some((world.get::<Name>(v)?.0.clone(), *world.get::<Transform>(v)?)))
        .collect();
    if let Some(mut r) = world.get_mut::<Rig>(rig) {
        if let Some(c) = r.clips.iter_mut().find(|c| c.name == clip_name) {
            for (n, t) in &poses {
                clip::set_key(c, n, time, t);
            }
        }
    }
}

/// Start a clip on a rig while playing (from the start).
pub fn play(world: &mut World, rig: Entity, clip_name: &str) {
    let now = world.try_resource::<kit_core::Time>().map_or(0.0, |t| t.elapsed);
    if let Some(mut rt) = world.try_resource_mut::<RigRuntime>() {
        rt.playing.insert(rig, (clip_name.to_owned(), now));
    }
}

pub fn stop(world: &mut World, rig: Entity) {
    if let Some(mut rt) = world.try_resource_mut::<RigRuntime>() {
        rt.playing.remove(&rig);
    }
}

/// Forget playback (Play stopped).
pub fn reset(world: &mut World) {
    if let Some(mut rt) = world.try_resource_mut::<RigRuntime>() {
        rt.playing.clear();
        rt.started = false;
    }
}

/// Hash numbers to ~1e-5 precision, so floating-point noise (a spinning
/// rig's volumes are rigid relative to it, but not bit-for-bit) doesn't
/// count as a change and re-deform the mesh every frame.
fn hash_f32s(h: &mut impl Hasher, v: &[f32]) {
    for x in v {
        ((*x as f64 * 1e5).round() as i64).hash(h);
    }
}

const PALETTE: [[f32; 3]; 8] = [
    [0.36, 0.86, 0.78],
    [0.98, 0.66, 0.25],
    [0.62, 0.52, 0.98],
    [0.98, 0.42, 0.55],
    [0.45, 0.72, 0.98],
    [0.72, 0.92, 0.38],
    [0.98, 0.85, 0.35],
    [0.85, 0.55, 0.95],
];

/// Play rig clips in the game (Update, play mode only).
pub fn animate_rigs(world: &mut World) {
    let now = world.try_resource::<kit_core::Time>().map_or(0.0, |t| t.elapsed);
    let start = world.try_resource::<RigRuntime>().is_some_and(|rt| !rt.started);
    if start {
        let auto: Vec<(Entity, String)> = world
            .storage::<Rig>()
            .map(|s| {
                s.iter()
                    .filter(|(_, r)| r.enabled && !r.autoplay.is_empty())
                    .map(|(e, r)| (e, r.autoplay.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let mut rt = world.resource_mut::<RigRuntime>();
        rt.started = true;
        for (e, clip) in auto {
            rt.playing.insert(e, (clip, now));
        }
    }
    let playing: Vec<(Entity, String, f64)> = world
        .try_resource::<RigRuntime>()
        .map(|rt| rt.playing.iter().map(|(e, (c, t))| (*e, c.clone(), *t)).collect())
        .unwrap_or_default();
    for (rig, name, started) in playing {
        let clip = world.get::<Rig>(rig).and_then(|r| r.clips.iter().find(|c| c.name == name).cloned());
        let Some(clip) = clip else { continue };
        let t = clip::clip_time(&clip, (now - started) as f32);
        pose_clip(world, rig, &name, t);
    }
}

/// Bind and deform every rig (PostUpdate, edit and play: the editor previews poses).
pub fn deform_rigs(world: &mut World) {
    let Some(mut rt) = world.remove_resource::<RigRuntime>() else { return };
    let preview = world.try_resource::<RigPreview>().map(|p| *p).unwrap_or_default();
    let res = world.try_resource::<Resources>().map(|r| r.clone());
    let rigs: Vec<(Entity, Rig, Model)> = world
        .storage::<Rig>()
        .map(|s| s.iter().filter_map(|(e, r)| Some((e, r.clone(), world.get_cloned::<Model>(e)?))).collect())
        .unwrap_or_default();
    let mut deformed = world.remove_resource::<Deformed>().unwrap_or_default();
    // Rigs that went away (or were turned off): back to the plain mesh.
    deformed.meshes.retain(|e, _| rigs.iter().any(|(r, settings, _)| r == e && settings.enabled));
    rt.bindings.retain(|e, _| rigs.iter().any(|(r, settings, _)| r == e && settings.enabled));

    for (rig, settings, model) in rigs {
        let volumes = volumes_of(world, rig);
        // New volumes rest where they are.
        for v in &volumes {
            if world.get::<RigVolume>(*v).is_some_and(|r| r.rest.is_none()) {
                let m = rig_space(world, rig, *v);
                world.get_mut::<RigVolume>(*v).unwrap().rest = Some(m);
            }
        }
        if !(preview.rig == Some(rig) && preview.fitting) {
            let mut changed = false;
            for &volume in &volumes {
                let Some(t) = world.get_cloned::<Transform>(volume) else { continue };
                let translation = constrain_translation(world, rig, volume, t.translation);
                if translation != t.translation {
                    world.insert(volume, Transform { translation, ..t });
                    changed = true;
                }
            }
            if changed {
                kit_scene::propagate_transforms(world);
            }
        }
        if !settings.enabled {
            continue;
        }
        let key = mesh_key(&model);
        let rest = rt.meshes.entry(key.clone()).or_insert_with(|| load_rest(&model, res.as_ref())).clone();
        let Some(rest) = rest else { continue };
        let bind: Vec<BindVolume> = volumes
            .iter()
            .map(|v| {
                let vol = world.get_cloned::<RigVolume>(*v).unwrap();
                BindVolume { rest: vol.rest.unwrap_or(Mat4::IDENTITY), volume: vol }
            })
            .collect();
        let bind_key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut h);
            for (v, b) in volumes.iter().zip(&bind) {
                v.to_bits().hash(&mut h);
                format!("{:?}", b.volume.shape).hash(&mut h);
                hash_f32s(&mut h, &[b.volume.influence, b.volume.falloff]);
                hash_f32s(&mut h, &b.rest.to_cols_array());
            }
            h.finish()
        };
        let motions: Vec<Motion> =
            volumes.iter().zip(&bind).map(|(v, b)| Motion::new(rig_space(world, rig, *v) * b.rest.inverse())).collect();
        let pose_key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            bind_key.hash(&mut h);
            settings.preserve_volume.hash(&mut h);
            for m in &motions {
                hash_f32s(&mut h, &m.full.to_cols_array());
            }
            let p = if preview.rig == Some(rig) { preview } else { RigPreview::default() };
            p.weights.hash(&mut h);
            p.focus.map(|e| e.to_bits()).hash(&mut h);
            h.finish()
        };
        let rebound = rt.bindings.get(&rig).is_none_or(|b| b.key != bind_key || b.mesh != key);
        if rebound {
            let weights = bind::weights(&rest.vertices, &bind);
            rt.bindings.insert(
                rig,
                Binding { key: bind_key, mesh: key.clone(), volumes: volumes.clone(), weights, pose_key: 0 },
            );
        }
        let binding = rt.bindings.get_mut(&rig).unwrap();
        if binding.pose_key == pose_key && deformed.meshes.contains_key(&rig) {
            continue;
        }
        binding.pose_key = pose_key;
        let mut vertices = Vec::new();
        if volumes.is_empty() {
            vertices = rest.vertices.clone();
        } else {
            deform::deform(&rest.vertices, &binding.weights, &motions, settings.preserve_volume, &mut vertices);
        }
        let colors = (preview.rig == Some(rig) && preview.weights).then(|| {
            let focus = preview.focus.and_then(|f| volumes.iter().position(|v| *v == f));
            binding
                .weights
                .iter()
                .map(|w| match focus {
                    Some(fi) => {
                        let weight = w.iter().filter(|(i, _)| *i as usize == fi).map(|(_, x)| *x).sum::<f32>();
                        // Cool slate → hot orange.
                        let cold = Vec3::new(0.16, 0.18, 0.32);
                        let hot = Vec3::new(1.0, 0.55, 0.18);
                        cold.lerp(hot, weight.powf(0.8)).to_array()
                    }
                    None => {
                        let mut c = Vec3::ZERO;
                        for (i, x) in w {
                            c += Vec3::from_array(PALETTE[*i as usize % PALETTE.len()]) * *x;
                        }
                        c.to_array()
                    }
                })
                .collect()
        });
        rt.revision += 1;
        deformed
            .meshes
            .insert(rig, DeformedMesh { revision: rt.revision, vertices, parts: rest.parts.clone(), colors });
    }
    world.insert_resource(deformed);
    world.insert_resource(rt);
}

/// Registers Rigg: clip playback (play mode) and binding/deformation (always).
pub struct RiggModule;

impl Module for RiggModule {
    fn name(&self) -> &'static str {
        "rigg"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["scene", "gamesys"]
    }
    fn build(&self, app: &mut App) {
        app.world.insert_resource(RigRuntime::default());
        app.world.insert_resource(Deformed::default());
        app.world.insert_resource(RigPreview::default());
        app.add_play_system(Stage::Update, "animate_rigs", animate_rigs);
        // After transforms are propagated, so volumes are where they'll be drawn.
        app.add_system(Stage::PostUpdate, "deform_rigs", deform_rigs);
        let _ = RunMode::Edit;
    }
}

#[cfg(test)]
mod tests;
