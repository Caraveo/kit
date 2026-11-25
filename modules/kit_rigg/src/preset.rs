//! Rig presets. The humanoid preset fits ~25 volumes to a model's bounds.

use glam::{Mat4, Quat, Vec3};
use kit_ecs::{Entity, World};
use kit_gamesys::{RigShape, RigVolume};
use kit_scene::{Name, Transform};

/// One volume of a preset, in rig space.
struct Part {
    name: &'static str,
    parent: Option<&'static str>,
    shape: RigShape,
    /// Rig-space transform.
    at: Mat4,
    falloff: f32,
}

/// A capsule from `a` to `b` (rig space).
fn capsule(name: &'static str, parent: Option<&'static str>, a: Vec3, b: Vec3, radius: f32, falloff: f32) -> Part {
    let d = b - a;
    let rot = Quat::from_rotation_arc(Vec3::Y, d.normalize_or(Vec3::Y));
    Part {
        name,
        parent,
        shape: RigShape::Capsule { radius, length: (d.length() - 2.0 * radius).max(radius * 0.5) },
        at: Mat4::from_rotation_translation(rot, (a + b) * 0.5),
        falloff,
    }
}

fn sphere(name: &'static str, parent: Option<&'static str>, at: Vec3, radius: f32, falloff: f32) -> Part {
    Part { name, parent, shape: RigShape::Sphere { radius }, at: Mat4::from_translation(at), falloff }
}

/// The humanoid layout for a model whose rest mesh spans `min`..`max` (rig
/// space, Y up). Arms are placed out to the sides when the model is wider
/// than ~55% of its height (T/A pose), else hanging down.
fn humanoid(min: Vec3, max: Vec3) -> Vec<Part> {
    let h = (max.y - min.y).max(1e-3);
    let c = (min + max) * 0.5;
    let y = |f: f32| min.y + f * h;
    let at = |x: f32, f: f32| Vec3::new(c.x + x * h, y(f), c.z);
    let width = max.x - min.x;
    let t_pose = width > 0.55 * h;
    let r = h * 0.035; // limb radius
    let fo = h * 0.06; // falloff
    let mut parts = vec![
        sphere("Hips", None, at(0.0, 0.53), h * 0.07, fo),
        capsule("Spine_01", Some("Hips"), at(0.0, 0.55), at(0.0, 0.62), h * 0.07, fo),
        capsule("Spine_02", Some("Spine_01"), at(0.0, 0.62), at(0.0, 0.69), h * 0.075, fo),
        capsule("Spine_03", Some("Spine_02"), at(0.0, 0.69), at(0.0, 0.76), h * 0.08, fo),
        Part {
            name: "Chest",
            parent: Some("Spine_03"),
            shape: RigShape::Box { size: Vec3::new(h * 0.2, h * 0.12, h * 0.11) },
            at: Mat4::from_translation(at(0.0, 0.74)),
            falloff: fo,
        },
        capsule("Neck", Some("Spine_03"), at(0.0, 0.82), at(0.0, 0.87), h * 0.03, fo * 0.7),
        sphere("Head", Some("Neck"), at(0.0, 0.93), h * 0.075, fo),
    ];
    for side in [-1.0f32, 1.0] {
        let (s, u, e, f, w, hand, th, kn, sh, an) = if side < 0.0 {
            ("L_Shoulder", "L_UpperArm", "L_Elbow", "L_Forearm", "L_Wrist", "L_Hand", "L_Thigh", "L_Knee", "L_Shin", "L_Ankle")
        } else {
            ("R_Shoulder", "R_UpperArm", "R_Elbow", "R_Forearm", "R_Wrist", "R_Hand", "R_Thigh", "R_Knee", "R_Shin", "R_Ankle")
        };
        let x = side;
        let shoulder = at(0.11 * x, 0.81);
        let (elbow, wrist, hand_at) = if t_pose {
            let reach = (width * 0.5 / h).clamp(0.3, 0.6);
            (at(0.11 * x + (reach - 0.11) * 0.45 * x, 0.80), at(0.11 * x + (reach - 0.11) * 0.88 * x, 0.79), at(reach * 0.97 * x, 0.79))
        } else {
            (at(0.16 * x, 0.64), at(0.18 * x, 0.49), at(0.185 * x, 0.44))
        };
        parts.push(sphere(s, Some("Spine_03"), shoulder, r * 1.4, fo * 0.8));
        parts.push(capsule(u, Some(s), shoulder, elbow, r, fo * 0.6));
        parts.push(sphere(e, Some(u), elbow, r * 1.1, fo * 0.6));
        parts.push(capsule(f, Some(e), elbow, wrist, r * 0.85, fo * 0.6));
        parts.push(sphere(w, Some(f), wrist, r * 0.9, fo * 0.5));
        parts.push(sphere(hand, Some(w), hand_at, r * 1.2, fo * 0.5));
        let hip = at(0.06 * x, 0.50);
        let knee = at(0.065 * x, 0.27);
        let ankle = at(0.07 * x, 0.05);
        parts.push(capsule(th, Some("Hips"), hip, knee, r * 1.5, fo * 0.7));
        parts.push(sphere(kn, Some(th), knee, r * 1.3, fo * 0.6));
        parts.push(capsule(sh, Some(kn), knee, ankle, r * 1.2, fo * 0.6));
        parts.push(sphere(an, Some(sh), ankle, r * 1.2, fo * 0.6));
    }
    parts
}

/// Add the humanoid preset under `rig` (a "Rig Volumes" folder object).
/// `bounds` is the rest mesh's (min, max) in rig space. Returns the folder.
pub fn add_humanoid(world: &mut World, rig: Entity, bounds: (Vec3, Vec3)) -> Entity {
    let folder = kit_scene::spawn_node(world, "Rig Volumes", Transform::IDENTITY);
    kit_scene::set_parent(world, folder, rig);
    let parts = humanoid(bounds.0, bounds.1);
    let mut made: Vec<(&str, Entity, Mat4)> = Vec::new();
    for p in parts {
        let (parent_e, parent_m) = p
            .parent
            .and_then(|name| made.iter().find(|(n, _, _)| *n == name).map(|(_, e, m)| (*e, *m)))
            .unwrap_or((folder, Mat4::IDENTITY));
        let local = Transform::from_matrix(parent_m.inverse() * p.at);
        let e = kit_scene::spawn_node(world, p.name, local);
        world.insert(e, RigVolume { shape: p.shape, influence: 1.0, falloff: p.falloff, rest: None, links: Vec::new() });
        kit_scene::set_parent(world, e, parent_e);
        made.push((p.name, e, p.at));
    }
    let _ = world.get::<Name>(folder);
    folder
}

/// A single volume added at a point (rig space) under `parent`.
pub fn add_volume(world: &mut World, parent: Entity, name: &str, shape: RigShape, local: Transform) -> Entity {
    let e = kit_scene::spawn_node(world, name, local);
    world.insert(e, RigVolume { shape, ..Default::default() });
    kit_scene::set_parent(world, e, parent);
    e
}

#[cfg(test)]
pub(crate) fn humanoid_names() -> Vec<&'static str> {
    humanoid(Vec3::ZERO, Vec3::new(1.0, 2.0, 0.4)).iter().map(|p| p.name).collect()
}
