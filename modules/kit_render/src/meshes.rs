//! Built-in mesh generation. All meshes are unit-sized and centered on the origin,
//! with counter-clockwise front faces.

use glam::{Vec2, Vec3};
use kit_gamesys::MeshKind;
use std::f32::consts::{PI, TAU};

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    fn vert(&mut self, p: Vec3, n: Vec3, uv: Vec2) -> u32 {
        self.vertices.push(Vertex { pos: p.into(), normal: n.into(), uv: uv.into() });
        self.vertices.len() as u32 - 1
    }

    /// Quad from four corners in counter-clockwise order (as seen from the front).
    fn quad(&mut self, corners: [Vec3; 4], n: Vec3) {
        let uvs = [Vec2::new(0.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 0.0)];
        let base = self.vertices.len() as u32;
        for (c, uv) in corners.iter().zip(uvs) {
            self.vert(*c, n, uv);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

pub fn build(kind: MeshKind) -> MeshData {
    match kind {
        MeshKind::Cube => cube(),
        MeshKind::Sphere => sphere(32, 20),
        MeshKind::Plane => plane(),
        MeshKind::Quad => quad(),
        MeshKind::Cylinder => cylinder(32),
        MeshKind::Torus => torus(48, 20, 0.35, 0.15),
    }
}

fn cube() -> MeshData {
    let mut m = MeshData::default();
    let h = 0.5;
    // (normal, right, up) per face; corners go bl, br, tr, tl.
    let faces = [
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
    ];
    for (n, r, u) in faces {
        let c = n * h;
        m.quad([c - r * h - u * h, c + r * h - u * h, c + r * h + u * h, c - r * h + u * h], n);
    }
    m
}

/// Unit plane on XZ facing +Y.
fn plane() -> MeshData {
    let mut m = MeshData::default();
    let h = 0.5;
    m.quad([Vec3::new(-h, 0.0, h), Vec3::new(h, 0.0, h), Vec3::new(h, 0.0, -h), Vec3::new(-h, 0.0, -h)], Vec3::Y);
    m
}

/// Unit quad on XY facing +Z (sprites).
fn quad() -> MeshData {
    let mut m = MeshData::default();
    let h = 0.5;
    m.quad([Vec3::new(-h, -h, 0.0), Vec3::new(h, -h, 0.0), Vec3::new(h, h, 0.0), Vec3::new(-h, h, 0.0)], Vec3::Z);
    m
}

fn sphere(segments: u32, rings: u32) -> MeshData {
    let mut m = MeshData::default();
    for r in 0..=rings {
        let v = r as f32 / rings as f32;
        let phi = v * PI;
        for s in 0..=segments {
            let u = s as f32 / segments as f32;
            let theta = u * TAU;
            let n = Vec3::new(phi.sin() * theta.cos(), phi.cos(), -phi.sin() * theta.sin());
            m.vert(n * 0.5, n, Vec2::new(u, v));
        }
    }
    let stride = segments + 1;
    for r in 0..rings {
        for s in 0..segments {
            let a = r * stride + s;
            let b = a + stride;
            m.indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    m
}

fn cylinder(segments: u32) -> MeshData {
    let mut m = MeshData::default();
    let (r, h) = (0.5, 0.5);
    let ring = |i: u32| {
        let t = i as f32 / segments as f32 * TAU;
        (Vec3::new(t.cos(), 0.0, -t.sin()), i as f32 / segments as f32)
    };
    // Side.
    let base = m.vertices.len() as u32;
    for i in 0..=segments {
        let (d, u) = ring(i);
        m.vert(d * r - Vec3::Y * h, d, Vec2::new(u, 1.0));
        m.vert(d * r + Vec3::Y * h, d, Vec2::new(u, 0.0));
    }
    for i in 0..segments {
        let a = base + i * 2;
        m.indices.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
    }
    // Caps.
    for (y, n) in [(h, Vec3::Y), (-h, Vec3::NEG_Y)] {
        let center = m.vert(Vec3::Y * y, n, Vec2::splat(0.5));
        let start = m.vertices.len() as u32;
        for i in 0..=segments {
            let (d, _) = ring(i);
            m.vert(d * r + Vec3::Y * y, n, Vec2::new(0.5 + d.x * 0.5, 0.5 - d.z * 0.5));
        }
        for i in 0..segments {
            let (a, b) = (start + i, start + i + 1);
            if n.y > 0.0 {
                m.indices.extend_from_slice(&[center, a, b]);
            } else {
                m.indices.extend_from_slice(&[center, b, a]);
            }
        }
    }
    m
}

fn torus(segments: u32, sides: u32, major: f32, minor: f32) -> MeshData {
    let mut m = MeshData::default();
    for i in 0..=segments {
        let u = i as f32 / segments as f32;
        let a = u * TAU;
        let dir = Vec3::new(a.cos(), 0.0, -a.sin());
        for j in 0..=sides {
            let v = j as f32 / sides as f32;
            let b = v * TAU;
            let n = dir * b.cos() + Vec3::Y * b.sin();
            m.vert(dir * major + n * minor, n, Vec2::new(u, v));
        }
    }
    let stride = sides + 1;
    for i in 0..segments {
        for j in 0..sides {
            let a = i * stride + j;
            let b = a + stride;
            m.indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle's winding should agree with its vertex normals (CCW front faces).
    #[test]
    fn windings_face_outward() {
        for kind in MeshKind::ALL {
            let m = build(kind);
            assert!(!m.indices.is_empty());
            let mut bad = 0;
            for t in m.indices.chunks(3) {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(m.vertices[i as usize].pos));
                let face_n = (b - a).cross(c - a);
                if face_n.length() < 1e-9 {
                    continue; // degenerate pole triangles
                }
                let vn: Vec3 = [t[0], t[1], t[2]].iter().map(|i| Vec3::from(m.vertices[*i as usize].normal)).sum();
                if face_n.dot(vn) <= 0.0 {
                    bad += 1;
                }
            }
            assert_eq!(bad, 0, "{kind:?} has {bad} inward-facing triangles");
        }
    }
}
