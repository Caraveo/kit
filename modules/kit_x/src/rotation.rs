//! The `Rotation` module and the vector-friendly `Math` helpers.
//!
//! Rotations in x are VECTORs of angles in degrees (X, then Y, then Z: the
//! same as `Transform.Rotation`). These functions do the careful math
//! (quaternions) underneath, so blends and turns take the shortest way round
//! and never get stuck at straight up or down.

use crate::value::Value;

#[derive(Clone, Copy, Debug)]
struct Quat {
    x: f64,
    y: f64,
    z: f64,
    w: f64,
}

type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn norm(a: V3) -> Option<V3> {
    let l = len(a);
    (l > 1e-12).then(|| [a[0] / l, a[1] / l, a[2] / l])
}
fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

impl Quat {
    const IDENTITY: Quat = Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    fn axis_angle(axis: V3, radians: f64) -> Quat {
        let Some(a) = norm(axis) else { return Quat::IDENTITY };
        let (s, c) = (radians * 0.5).sin_cos();
        Quat { x: a[0] * s, y: a[1] * s, z: a[2] * s, w: c }
    }

    fn mul(self, o: Quat) -> Quat {
        Quat {
            w: self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            x: self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            y: self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            z: self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        }
    }

    fn dot(self, o: Quat) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    fn normalize(self) -> Quat {
        let l = self.dot(self).sqrt();
        if l < 1e-12 { Quat::IDENTITY } else { Quat { x: self.x / l, y: self.y / l, z: self.z / l, w: self.w / l } }
    }

    fn inverse(self) -> Quat {
        Quat { x: -self.x, y: -self.y, z: -self.z, w: self.w }
    }

    fn rotate(self, v: V3) -> V3 {
        let q = [self.x, self.y, self.z];
        let t = scale(cross(q, v), 2.0);
        let c = cross(q, t);
        [v[0] + self.w * t[0] + c[0], v[1] + self.w * t[1] + c[1], v[2] + self.w * t[2] + c[2]]
    }

    /// Degrees, X then Y then Z (q = qx * qy * qz), like Kit's transforms.
    fn from_degrees(d: V3) -> Quat {
        let qx = Quat::axis_angle([1.0, 0.0, 0.0], d[0].to_radians());
        let qy = Quat::axis_angle([0.0, 1.0, 0.0], d[1].to_radians());
        let qz = Quat::axis_angle([0.0, 0.0, 1.0], d[2].to_radians());
        qx.mul(qy).mul(qz)
    }

    fn to_degrees(self) -> V3 {
        // Columns of the rotation matrix.
        let r = self.rotate([1.0, 0.0, 0.0]);
        let u = self.rotate([0.0, 1.0, 0.0]);
        let b = self.rotate([0.0, 0.0, 1.0]);
        // M = Rx * Ry * Rz: M02 = sin(y), M12 = -sin(x)cos(y), M22 = cos(x)cos(y), M01 = -cos(y)sin(z), M00 = cos(y)cos(z)
        let y = b[0].clamp(-1.0, 1.0).asin();
        let (x, z) =
            if b[0].abs() < 0.999_999 { ((-b[1]).atan2(b[2]), (-u[0]).atan2(r[0])) } else { (u[2].atan2(u[1]), 0.0) };
        [x.to_degrees(), y.to_degrees(), z.to_degrees()]
    }

    fn slerp(self, mut o: Quat, t: f64) -> Quat {
        let mut d = self.dot(o);
        if d < 0.0 {
            o = Quat { x: -o.x, y: -o.y, z: -o.z, w: -o.w };
            d = -d;
        }
        if d > 0.9995 {
            let l = |a: f64, b: f64| a + (b - a) * t;
            return Quat { x: l(self.x, o.x), y: l(self.y, o.y), z: l(self.z, o.z), w: l(self.w, o.w) }.normalize();
        }
        let theta = d.acos();
        let s = theta.sin();
        let (a, b) = (((1.0 - t) * theta).sin() / s, (t * theta).sin() / s);
        Quat { x: a * self.x + b * o.x, y: a * self.y + b * o.y, z: a * self.z + b * o.z, w: a * self.w + b * o.w }
            .normalize()
    }

    /// Angle between two rotations, in degrees.
    fn angle_to(self, o: Quat) -> f64 {
        (2.0 * self.dot(o).abs().min(1.0).acos()).to_degrees()
    }

    /// Forward (-Z) along `dir`, with `up` kept as up as it can be.
    fn look(dir: V3, up: V3) -> Option<Quat> {
        let f = norm(dir)?;
        let right = norm(cross(f, up))
            .or_else(|| norm(cross(f, if f[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] })))?;
        let up = cross(right, f);
        let back = scale(f, -1.0);
        // Rotation matrix with columns (right, up, back) → quaternion.
        let (m00, m01, m02) = (right[0], up[0], back[0]);
        let (m10, m11, m12) = (right[1], up[1], back[1]);
        let (m20, m21, m22) = (right[2], up[2], back[2]);
        let trace = m00 + m11 + m22;
        let q = if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            Quat { w: 0.25 * s, x: (m21 - m12) / s, y: (m02 - m20) / s, z: (m10 - m01) / s }
        } else if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            Quat { w: (m21 - m12) / s, x: 0.25 * s, y: (m01 + m10) / s, z: (m02 + m20) / s }
        } else if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            Quat { w: (m02 - m20) / s, x: (m01 + m10) / s, y: 0.25 * s, z: (m12 + m21) / s }
        } else {
            let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            Quat { w: (m10 - m01) / s, x: (m02 + m20) / s, y: (m12 + m21) / s, z: 0.25 * s }
        };
        Some(q.normalize())
    }

    /// The smallest rotation turning direction `a` to direction `b`.
    fn between(a: V3, b: V3) -> Option<Quat> {
        let (a, b) = (norm(a)?, norm(b)?);
        let d = dot(a, b);
        if d < -0.999_999 {
            // Opposite: half a turn around any perpendicular axis.
            let axis = norm(cross([1.0, 0.0, 0.0], a)).or_else(|| norm(cross([0.0, 1.0, 0.0], a)))?;
            return Some(Quat::axis_angle(axis, std::f64::consts::PI));
        }
        let c = cross(a, b);
        Some(Quat { x: c[0], y: c[1], z: c[2], w: 1.0 + d }.normalize())
    }
}

fn vec(v: &Value, what: &str, f: &str) -> Result<V3, String> {
    v.as_vector().ok_or_else(|| format!("Rotation.{f}: {what} must be a VECTOR"))
}

fn num(v: Option<&Value>, what: &str, f: &str) -> Result<f64, String> {
    v.and_then(Value::as_f64).ok_or_else(|| format!("Rotation.{f}: {what} must be a number"))
}

fn out(q: Quat) -> Value {
    let d = q.to_degrees();
    Value::Vector(d)
}

/// `Rotation.Member(args)`.
pub fn rotation(name: &str, a: &[Value]) -> Result<Value, String> {
    let arg = |i: usize, what: &str| a.get(i).ok_or_else(|| format!("Rotation.{name}: missing {what}"));
    let rot =
        |i: usize, what: &str| -> Result<Quat, String> { Ok(Quat::from_degrees(vec(arg(i, what)?, what, name)?)) };
    match name {
        // Blend two rotations (0 = first, 1 = second) the shortest way round.
        "Blend" => Ok(out(
            rot(0, "the first rotation")?.slerp(rot(1, "the second rotation")?, num(a.get(2), "the amount", name)?)
        )),
        // Rotate by `first`, then by `second`.
        "Combine" => Ok(out(rot(1, "the second rotation")?.mul(rot(0, "the first rotation")?).normalize())),
        // The rotation that undoes this one.
        "Reverse" => Ok(out(rot(0, "the rotation")?.inverse())),
        // Turn a vector by a rotation.
        "Apply" => Ok(Value::Vector(rot(0, "the rotation")?.rotate(vec(arg(1, "the vector")?, "the vector", name)?))),
        // Face along a direction (with an optional up).
        "LookToward" => {
            let dir = vec(arg(0, "the direction")?, "the direction", name)?;
            let up = match a.get(1) {
                Some(u) => vec(u, "up", name)?,
                None => [0.0, 1.0, 0.0],
            };
            Ok(Quat::look(dir, up).map(out).unwrap_or(Value::Vector([0.0; 3])))
        }
        // Turn around an axis by some degrees.
        "AroundAxis" => {
            let axis = vec(arg(0, "the axis")?, "the axis", name)?;
            Ok(out(Quat::axis_angle(axis, num(a.get(1), "the degrees", name)?.to_radians())))
        }
        // The turn that takes one direction to another.
        "Between" => {
            let from = vec(arg(0, "the first direction")?, "the first direction", name)?;
            let to = vec(arg(1, "the second direction")?, "the second direction", name)?;
            Ok(Quat::between(from, to).map(out).unwrap_or(Value::Vector([0.0; 3])))
        }
        // How far apart two rotations are, in degrees.
        "Angle" => Ok(Value::Float(rot(0, "the first rotation")?.angle_to(rot(1, "the second rotation")?))),
        // Turn from one rotation toward another by at most some degrees.
        "TurnToward" => {
            let (from, to) = (rot(0, "the current rotation")?, rot(1, "the target rotation")?);
            let max = num(a.get(2), "the most degrees", name)?.max(0.0);
            let angle = from.angle_to(to);
            Ok(out(if angle <= max || angle < 1e-9 { to } else { from.slerp(to, max / angle) }))
        }
        // Which way a rotation faces.
        "Forward" => Ok(Value::Vector(rot(0, "the rotation")?.rotate([0.0, 0.0, -1.0]))),
        "Up" => Ok(Value::Vector(rot(0, "the rotation")?.rotate([0.0, 1.0, 0.0]))),
        "Right" => Ok(Value::Vector(rot(0, "the rotation")?.rotate([1.0, 0.0, 0.0]))),
        _ => Err(format!("Rotation has no member `{name}`")),
    }
}

/// Smooth noise from 0 to 1 (the same input always gives the same output).
pub fn noise(p: V3, seed: u64) -> f64 {
    fn hash(x: i64, y: i64, z: i64, seed: u64) -> f64 {
        let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
            ^ seed.wrapping_mul(0x27D4_EB2F_1656_67C5);
        h ^= h >> 31;
        h = h.wrapping_mul(0x7FB5_D329_728E_A185);
        h ^= h >> 27;
        (h >> 11) as f64 / (1u64 << 53) as f64
    }
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
    let c = [p[0].floor(), p[1].floor(), p[2].floor()];
    let f = [smooth(p[0] - c[0]), smooth(p[1] - c[1]), smooth(p[2] - c[2])];
    let (x, y, z) = (c[0] as i64, c[1] as i64, c[2] as i64);
    let l = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let h = |dx, dy, dz| hash(x + dx, y + dy, z + dz, seed);
    l(
        l(l(h(0, 0, 0), h(1, 0, 0), f[0]), l(h(0, 1, 0), h(1, 1, 0), f[0]), f[1]),
        l(l(h(0, 0, 1), h(1, 0, 1), f[0]), l(h(0, 1, 1), h(1, 1, 1), f[0]), f[1]),
        f[2],
    )
}
