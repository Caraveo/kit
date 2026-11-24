//! Terrain: a height grid with procedural generation and sculpting.
//!
//! The grid is centered on the entity: `size` world units wide along X and Z,
//! with `resolution × resolution` height samples. Row `z` / column `x` is
//! sample `heights[z * resolution + x]`, at local position
//! `(-size/2 + x * cell, height, -size/2 + z * cell)`.

use glam::{Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};

pub const RESOLUTIONS: [u32; 4] = [33, 65, 129, 257];

/// Sculpting brushes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SculptTool {
    Raise,
    Lower,
    Smooth,
    Flatten,
}

impl SculptTool {
    pub const ALL: [SculptTool; 4] = [SculptTool::Raise, SculptTool::Lower, SculptTool::Smooth, SculptTool::Flatten];
    pub fn label(self) -> &'static str {
        match self {
            SculptTool::Raise => "Raise",
            SculptTool::Lower => "Lower",
            SculptTool::Smooth => "Smooth",
            SculptTool::Flatten => "Flatten",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Terrain {
    /// Width and depth in world units.
    pub size: f32,
    /// Samples per side (see [`RESOLUTIONS`]).
    pub resolution: u32,
    /// Heights in world units, row-major (z rows of x samples).
    #[serde(with = "compact_heights")]
    pub heights: Vec<f32>,
    pub sand: Vec4,
    pub grass: Vec4,
    pub rock: Vec4,
    pub snow: Vec4,
    /// Heights below this are sandy.
    pub sand_height: f32,
    /// Heights above this get snow.
    pub snow_height: f32,
    /// Give the terrain a collider (physics objects stand on it).
    pub collide: bool,
}

impl Default for Terrain {
    fn default() -> Self {
        Self::flat(64.0, 129)
    }
}

impl Terrain {
    pub fn flat(size: f32, resolution: u32) -> Self {
        let r = resolution.max(2);
        Self {
            size,
            resolution: r,
            heights: vec![0.0; (r * r) as usize],
            sand: Vec4::new(0.86, 0.80, 0.62, 1.0),
            grass: Vec4::new(0.42, 0.62, 0.36, 1.0),
            rock: Vec4::new(0.50, 0.46, 0.52, 1.0),
            snow: Vec4::new(0.96, 0.96, 1.0, 1.0),
            sand_height: -0.6,
            snow_height: 7.0,
            collide: true,
        }
    }

    /// Make sure `heights` matches `resolution` (after hand-editing a file).
    pub fn validate(&mut self) {
        self.resolution = self.resolution.clamp(2, 1025);
        let n = (self.resolution * self.resolution) as usize;
        if self.heights.len() != n {
            self.heights.resize(n, 0.0);
        }
        self.size = self.size.max(0.1);
    }

    pub fn cell(&self) -> f32 {
        self.size / (self.resolution - 1) as f32
    }

    fn idx(&self, x: u32, z: u32) -> usize {
        (z * self.resolution + x) as usize
    }

    pub fn sample(&self, x: u32, z: u32) -> f32 {
        let r = self.resolution - 1;
        self.heights[self.idx(x.min(r), z.min(r))]
    }

    /// Local position of a grid sample.
    pub fn vertex(&self, x: u32, z: u32) -> Vec3 {
        let half = self.size * 0.5;
        let c = self.cell();
        Vec3::new(-half + x as f32 * c, self.sample(x, z), -half + z as f32 * c)
    }

    /// Normal at a grid sample (central differences).
    pub fn normal(&self, x: u32, z: u32) -> Vec3 {
        let c = self.cell();
        let l = self.sample(x.saturating_sub(1), z);
        let r = self.sample(x + 1, z);
        let d = self.sample(x, z.saturating_sub(1));
        let u = self.sample(x, z + 1);
        Vec3::new((l - r) / (2.0 * c), 1.0, (d - u) / (2.0 * c)).normalize()
    }

    /// Height at a local (x, z) position, or None outside the terrain.
    pub fn height_at(&self, local: Vec2) -> Option<f32> {
        let half = self.size * 0.5;
        let c = self.cell();
        let fx = (local.x + half) / c;
        let fz = (local.y + half) / c;
        let max = (self.resolution - 1) as f32;
        if !(0.0..=max).contains(&fx) || !(0.0..=max).contains(&fz) {
            return None;
        }
        let (x0, z0) = (fx.floor() as u32, fz.floor() as u32);
        let (tx, tz) = (fx - x0 as f32, fz - z0 as f32);
        let h00 = self.sample(x0, z0);
        let h10 = self.sample(x0 + 1, z0);
        let h01 = self.sample(x0, z0 + 1);
        let h11 = self.sample(x0 + 1, z0 + 1);
        let a = h00 + (h10 - h00) * tx;
        let b = h01 + (h11 - h01) * tx;
        Some(a + (b - a) * tz)
    }

    /// Distance along a local-space ray to the surface.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        // Clip the ray to the terrain's bounding box first.
        let half = self.size * 0.5;
        let (lo, hi) = self.height_range();
        let bmin = Vec3::new(-half, lo - 0.01, -half);
        let bmax = Vec3::new(half, hi + 0.01, half);
        let (mut t0, mut t1) = (0.0f32, max);
        for i in 0..3 {
            if dir[i].abs() < 1e-8 {
                if origin[i] < bmin[i] || origin[i] > bmax[i] {
                    return None;
                }
            } else {
                let a = (bmin[i] - origin[i]) / dir[i];
                let b = (bmax[i] - origin[i]) / dir[i];
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        if t1 < t0 {
            return None;
        }
        let above = |t: f32| {
            let p = origin + dir * t;
            self.height_at(Vec2::new(p.x, p.z)).map(|h| p.y - h)
        };
        let step = (self.cell() * 0.5).max(1e-3);
        let mut t = t0;
        let mut prev = above(t)?;
        if prev < 0.0 {
            return Some(t);
        }
        while t < t1 {
            let next_t = (t + step).min(t1);
            let Some(d) = above(next_t) else { break };
            if d <= 0.0 {
                // Refine between t and next_t.
                let (mut a, mut b) = (t, next_t);
                for _ in 0..16 {
                    let m = (a + b) * 0.5;
                    if above(m).unwrap_or(0.0) > 0.0 { a = m } else { b = m }
                }
                return Some((a + b) * 0.5);
            }
            prev = d;
            t = next_t;
            if t >= t1 {
                break;
            }
        }
        let _ = prev;
        None
    }

    pub fn height_range(&self) -> (f32, f32) {
        self.heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(*h), hi.max(*h)))
    }

    /// Fill with rolling hills: fractal value noise.
    pub fn generate(&mut self, seed: u32, height: f32, roughness: f32) {
        let r = self.resolution;
        let base = 3.0 / self.size.max(1.0) * (self.size / 64.0).sqrt();
        let rough = roughness.clamp(0.0, 1.0);
        for z in 0..r {
            for x in 0..r {
                let p = Vec2::new(x as f32, z as f32) * self.cell();
                let mut amp = 1.0;
                let mut freq = base;
                let mut sum = 0.0;
                let mut norm = 0.0;
                for octave in 0..6 {
                    sum += value_noise(p * freq, seed.wrapping_add(octave * 7919)) * amp;
                    norm += amp;
                    amp *= 0.35 + rough * 0.3;
                    freq *= 2.0;
                }
                let n = sum / norm; // 0..1
                // Valleys flatter, peaks sharper.
                let i = self.idx(x, z);
                self.heights[i] = (n * n * 1.6 - 0.15).max(0.0) * height;
            }
        }
    }

    pub fn flatten_all(&mut self, h: f32) {
        self.heights.iter_mut().for_each(|v| *v = h);
    }

    /// Resample to a new resolution, keeping the shape.
    pub fn resample(&mut self, resolution: u32) {
        let resolution = resolution.clamp(2, 1025);
        if resolution == self.resolution {
            return;
        }
        let old = self.clone();
        let half = self.size * 0.5;
        let c = self.size / (resolution - 1) as f32;
        let mut heights = Vec::with_capacity((resolution * resolution) as usize);
        for z in 0..resolution {
            for x in 0..resolution {
                let p = Vec2::new(-half + x as f32 * c, -half + z as f32 * c);
                heights.push(old.height_at(p).unwrap_or(0.0));
            }
        }
        self.resolution = resolution;
        self.heights = heights;
    }

    /// Apply a brush at a local (x, z) position. `amount` is the strength for
    /// this frame; `target` is the flatten height.
    pub fn sculpt(&mut self, tool: SculptTool, center: Vec2, radius: f32, amount: f32, target: f32) {
        let r = self.resolution;
        let half = self.size * 0.5;
        let c = self.cell();
        let radius = radius.max(c);
        let x0 = (((center.x - radius + half) / c).floor().max(0.0)) as u32;
        let x1 = (((center.x + radius + half) / c).ceil().min((r - 1) as f32)) as u32;
        let z0 = (((center.y - radius + half) / c).floor().max(0.0)) as u32;
        let z1 = (((center.y + radius + half) / c).ceil().min((r - 1) as f32)) as u32;
        if x0 > x1 || z0 > z1 {
            return;
        }
        let snapshot = if tool == SculptTool::Smooth { Some(self.heights.clone()) } else { None };
        for z in z0..=z1 {
            for x in x0..=x1 {
                let p = Vec2::new(-half + x as f32 * c, -half + z as f32 * c);
                let d = p.distance(center) / radius;
                if d >= 1.0 {
                    continue;
                }
                // Smooth falloff: 1 at the center, 0 at the edge.
                let f = {
                    let t = 1.0 - d;
                    t * t * (3.0 - 2.0 * t)
                };
                let i = self.idx(x, z);
                let h = self.heights[i];
                self.heights[i] = match tool {
                    SculptTool::Raise => h + amount * f,
                    SculptTool::Lower => h - amount * f,
                    SculptTool::Flatten => h + (target - h) * (amount * f).clamp(0.0, 1.0),
                    SculptTool::Smooth => {
                        let s = snapshot.as_ref().unwrap();
                        let get = |xx: u32, zz: u32| s[(zz.min(r - 1) * r + xx.min(r - 1)) as usize];
                        let avg =
                            (get(x.saturating_sub(1), z) + get(x + 1, z) + get(x, z.saturating_sub(1)) + get(x, z + 1))
                                * 0.25;
                        h + (avg - h) * (amount * f).clamp(0.0, 1.0)
                    }
                };
            }
        }
    }

    /// Cheap fingerprint of the heights (for change detection).
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (self.resolution as u64) ^ ((self.size.to_bits() as u64) << 32);
        for v in &self.heights {
            h = (h ^ v.to_bits() as u64).wrapping_mul(0x100_0000_01b3);
        }
        for c in [self.sand, self.grass, self.rock, self.snow] {
            for v in c.to_array() {
                h = (h ^ v.to_bits() as u64).wrapping_mul(0x100_0000_01b3);
            }
        }
        (h ^ self.sand_height.to_bits() as u64 ^ ((self.snow_height.to_bits() as u64) << 20)).wrapping_mul(31)
    }

    /// Surface color for a height and slope (`up` = normal.y), sRGB.
    pub fn color_for(&self, height: f32, up: f32) -> Vec4 {
        let smooth = |e0: f32, e1: f32, x: f32| {
            let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let mut c = self.sand.lerp(self.grass, smooth(self.sand_height, self.sand_height + 0.5, height));
        // Steep slopes are rocky.
        c = c.lerp(self.rock, smooth(0.82, 0.62, up));
        c.lerp(self.snow, smooth(self.snow_height, self.snow_height + 1.0, height) * smooth(0.55, 0.75, up))
    }
}

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h =
        (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 0x00ff_ffff as f32
}

fn value_noise(p: Vec2, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let (x, y) = (i.x as i32, i.y as i32);
    let s = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let a = hash2(x, y, seed);
    let b = hash2(x + 1, y, seed);
    let c = hash2(x, y + 1, seed);
    let d = hash2(x + 1, y + 1, seed);
    let ab = a + (b - a) * s.x;
    let cd = c + (d - c) * s.x;
    ab + (cd - ab) * s.y
}

/// Heights are stored rounded to millimeters to keep scene files small.
mod compact_heights {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &[f32], s: S) -> Result<S::Ok, S::Error> {
        let rounded: Vec<f32> = v.iter().map(|h| (h * 1000.0).round() / 1000.0).collect();
        rounded.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f32>, D::Error> {
        Vec::<f32>::deserialize(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_and_rays() {
        let mut t = Terrain::flat(10.0, 11);
        assert_eq!(t.height_at(Vec2::ZERO), Some(0.0));
        assert_eq!(t.height_at(Vec2::new(6.0, 0.0)), None);
        t.flatten_all(2.0);
        let d = t.raycast(Vec3::new(1.0, 10.0, 1.0), Vec3::NEG_Y, 100.0).unwrap();
        assert!((d - 8.0).abs() < 1e-3, "{d}");
        assert!(t.raycast(Vec3::new(1.0, 10.0, 1.0), Vec3::Y, 100.0).is_none());
        // Slanted ray onto a slope.
        for z in 0..11 {
            for x in 0..11 {
                let i = t.idx(x, z);
                t.heights[i] = x as f32; // rises 1 per unit along x
            }
        }
        assert!((t.height_at(Vec2::new(0.5, 0.0)).unwrap() - 5.5).abs() < 1e-4);
        let d = t.raycast(Vec3::new(-5.0, 20.0, 0.0), Vec3::new(1.0, -1.0, 0.0), 100.0).unwrap();
        let hit = Vec3::new(-5.0, 20.0, 0.0) + Vec3::new(1.0, -1.0, 0.0).normalize() * d;
        assert!((hit.y - t.height_at(Vec2::new(hit.x, hit.z)).unwrap()).abs() < 0.01);
    }

    #[test]
    fn sculpt_tools() {
        let mut t = Terrain::flat(20.0, 21);
        t.sculpt(SculptTool::Raise, Vec2::ZERO, 3.0, 1.0, 0.0);
        let center = t.height_at(Vec2::ZERO).unwrap();
        assert!((center - 1.0).abs() < 1e-4);
        assert!(t.height_at(Vec2::new(2.0, 0.0)).unwrap() < center);
        assert_eq!(t.height_at(Vec2::new(5.0, 0.0)).unwrap(), 0.0);
        for _ in 0..30 {
            t.sculpt(SculptTool::Smooth, Vec2::ZERO, 5.0, 1.0, 0.0);
        }
        assert!(t.height_at(Vec2::ZERO).unwrap() < center * 0.7, "smoothing spreads the bump");
        for _ in 0..50 {
            t.sculpt(SculptTool::Flatten, Vec2::ZERO, 6.0, 0.5, 0.25);
        }
        assert!((t.height_at(Vec2::ZERO).unwrap() - 0.25).abs() < 0.01);
    }

    #[test]
    fn generate_and_resample() {
        let mut t = Terrain::flat(64.0, 65);
        t.generate(7, 8.0, 0.5);
        let (lo, hi) = t.height_range();
        assert!(lo >= 0.0 && hi > 1.0 && hi <= 8.0 * 1.5, "{lo}..{hi}");
        let before = t.height_at(Vec2::new(3.3, -7.1)).unwrap();
        t.resample(129);
        assert_eq!(t.heights.len(), 129 * 129);
        assert!((t.height_at(Vec2::new(3.3, -7.1)).unwrap() - before).abs() < 0.2);
        let mut u = Terrain::flat(64.0, 65);
        u.generate(7, 8.0, 0.5);
        u.resample(129);
        assert_eq!(t.fingerprint(), u.fingerprint(), "deterministic");
    }

    #[test]
    fn colors_follow_height_and_slope() {
        let t = Terrain::default();
        assert_eq!(t.color_for(5.0, 1.0), t.grass);
        assert_eq!(t.color_for(0.0, 1.0), t.grass, "flat ground at 0 is grass");
        assert_eq!(t.color_for(-1.0, 1.0), t.sand, "dug below the sand line");
        assert_eq!(t.color_for(5.0, 0.3), t.rock);
        assert_eq!(t.color_for(20.0, 1.0), t.snow);
    }
}
