//! Binding: smooth, normalized influence weights from the volumes' shapes
//! in the rest pose.

use glam::{Mat4, Vec3};
use kit_gamesys::RigVolume;
use kit_render::Vertex;
use std::collections::HashMap;

/// Up to this many volumes move each vertex.
pub const MAX_INFLUENCES: usize = 4;

/// Weights for one vertex: (volume index, weight), summing to 1.
pub type Influences = [(u16, f32); MAX_INFLUENCES];

/// A volume as the binder sees it.
#[derive(Clone, Debug)]
pub struct BindVolume {
    pub volume: RigVolume,
    /// Rest transform in rig space.
    pub rest: Mat4,
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How strongly one volume pulls a point (rig space), before normalizing.
/// Inside: 1, rising toward 2 the deeper the point (so the volume a point is
/// most inside of wins overlaps). Outside: a smooth fade to 0 at `falloff`.
pub fn pull(v: &BindVolume, rest_inv: &Mat4, p: Vec3) -> f32 {
    let (scale, _, _) = v.rest.to_scale_rotation_translation();
    let s = (scale.x.abs() + scale.y.abs() + scale.z.abs()) / 3.0;
    let local = rest_inv.transform_point3(p);
    let d = v.volume.shape.distance(local) * s;
    let raw = if d <= 0.0 {
        1.0 + (-d / (v.volume.shape.extent() * s)).min(1.0)
    } else {
        let falloff = v.volume.falloff.max(1e-4);
        smoothstep(1.0 - d / falloff)
    };
    raw * v.volume.influence.clamp(0.0, 1.0)
}

/// Weights for every vertex. Vertices at the same position share weights (so
/// the triangle soup never tears). A vertex no volume reaches follows the
/// nearest one.
pub fn weights(vertices: &[Vertex], volumes: &[BindVolume]) -> Vec<Influences> {
    let empty = [(0u16, 0.0f32); MAX_INFLUENCES];
    if volumes.is_empty() {
        return vec![empty; vertices.len()];
    }
    let inv: Vec<Mat4> = volumes.iter().map(|v| v.rest.inverse()).collect();
    let mut cache: HashMap<[i32; 3], Influences> = HashMap::new();
    vertices
        .iter()
        .map(|v| {
            let p = Vec3::from_array(v.pos);
            let key = [(p.x * 1e4).round() as i32, (p.y * 1e4).round() as i32, (p.z * 1e4).round() as i32];
            *cache.entry(key).or_insert_with(|| influences_at(p, volumes, &inv))
        })
        .collect()
}

fn influences_at(p: Vec3, volumes: &[BindVolume], inv: &[Mat4]) -> Influences {
    let mut all: Vec<(u16, f32)> =
        volumes.iter().zip(inv).enumerate().map(|(i, (v, inv))| (i as u16, pull(v, inv, p))).filter(|(_, w)| *w > 1e-5).collect();
    if all.is_empty() {
        // Out of reach of every volume: follow the nearest surface.
        let nearest = volumes
            .iter()
            .zip(inv)
            .enumerate()
            .map(|(i, (v, inv))| (i, v.volume.shape.distance(inv.transform_point3(p))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0, |(i, _)| i);
        all.push((nearest as u16, 1.0));
    }
    all.sort_by(|a, b| b.1.total_cmp(&a.1));
    all.truncate(MAX_INFLUENCES);
    let total: f32 = all.iter().map(|(_, w)| w).sum();
    let mut out = [(0u16, 0.0f32); MAX_INFLUENCES];
    for (slot, (i, w)) in out.iter_mut().zip(all) {
        *slot = (i, w / total);
    }
    out
}
