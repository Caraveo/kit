//! Deforming the bound mesh by the volumes' movement since the rest pose.
//!
//! Each volume contributes `M = current · rest⁻¹`. Vertices blend those with
//! their weights, either linearly or with dual quaternions ("Preserve
//! Volume"), which keeps limbs from thinning where they twist or bend.

use crate::bind::Influences;
use glam::{Mat3, Mat4, Quat, Vec3};
use kit_render::Vertex;

/// One volume's movement, ready to blend.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    pub full: Mat4,
    pub normal: Mat3,
    pub rotation: Quat,
    /// Dual part of the rigid motion (translation encoded with the rotation).
    pub dual: Quat,
    /// The motion has scale (handled as a linear correction).
    pub scaled: bool,
    pub scale: Vec3,
}

impl Motion {
    pub fn new(m: Mat4) -> Self {
        let (scale, rotation, translation) = m.to_scale_rotation_translation();
        let rotation = rotation.normalize();
        let t = Quat::from_xyzw(translation.x, translation.y, translation.z, 0.0);
        let dual = (t * rotation) * 0.5;
        let normal = Mat3::from_mat4(m).inverse().transpose();
        Self { full: m, normal, rotation, dual, scaled: (scale - Vec3::ONE).abs().max_element() > 1e-4, scale }
    }

    pub fn identity() -> Self {
        Self::new(Mat4::IDENTITY)
    }
}

/// Deform `rest` into `out` (same length).
pub fn deform(rest: &[Vertex], weights: &[Influences], motions: &[Motion], preserve_volume: bool, out: &mut Vec<Vertex>) {
    out.clear();
    out.reserve(rest.len());
    for (v, w) in rest.iter().zip(weights) {
        let p = Vec3::from_array(v.pos);
        let n = Vec3::from_array(v.normal);
        let (pos, normal) = if preserve_volume { dual_quaternion(p, n, w, motions) } else { linear(p, n, w, motions) };
        out.push(Vertex { pos: pos.to_array(), normal: normal.normalize_or(n).to_array(), uv: v.uv });
    }
}

fn linear(p: Vec3, n: Vec3, w: &Influences, motions: &[Motion]) -> (Vec3, Vec3) {
    let (mut pos, mut normal) = (Vec3::ZERO, Vec3::ZERO);
    let mut total = 0.0;
    for &(i, weight) in w {
        if weight <= 0.0 {
            continue;
        }
        let m = &motions[i as usize];
        pos += m.full.transform_point3(p) * weight;
        normal += (m.normal * n) * weight;
        total += weight;
    }
    if total <= 0.0 { (p, n) } else { (pos, normal) }
}

fn dual_quaternion(p: Vec3, n: Vec3, w: &Influences, motions: &[Motion]) -> (Vec3, Vec3) {
    let mut real = Quat::from_xyzw(0.0, 0.0, 0.0, 0.0);
    let mut dual = Quat::from_xyzw(0.0, 0.0, 0.0, 0.0);
    let mut scale_fix = Vec3::ZERO;
    let mut first: Option<Quat> = None;
    let mut total = 0.0;
    for &(i, weight) in w {
        if weight <= 0.0 {
            continue;
        }
        let m = &motions[i as usize];
        // Keep all rotations in the same hemisphere (shortest blend).
        let sign = match first {
            Some(f) if f.dot(m.rotation) < 0.0 => -1.0,
            Some(_) => 1.0,
            None => {
                first = Some(m.rotation);
                1.0
            }
        };
        real = real + m.rotation * (weight * sign);
        dual = dual + m.dual * (weight * sign);
        if m.scaled {
            // Scale isn't rigid: add the difference a linear blend would make.
            let rigid = m.rotation * p + translation_of(m);
            scale_fix += (m.full.transform_point3(p) - rigid) * weight;
        }
        total += weight;
    }
    if total <= 0.0 {
        return (p, n);
    }
    let len = real.length();
    if len < 1e-6 {
        return linear(p, n, w, motions);
    }
    let (real, dual) = (real / len, dual / len);
    let t = dual * real.conjugate() * 2.0;
    let pos = real * p + Vec3::new(t.x, t.y, t.z) + scale_fix;
    (pos, real * n)
}

fn translation_of(m: &Motion) -> Vec3 {
    let t = m.dual * m.rotation.conjugate() * 2.0;
    Vec3::new(t.x, t.y, t.z)
}
