//! Kit Realtime wire protocol v1 (see `../PROTOCOL.md`). Must match the
//! Worker's `src/realtime/protocol.ts` byte for byte.

use glam::{Quat, Vec3};

pub const VERSION: u8 = 1;
pub const SUBPROTOCOL: &str = "kit-realtime.v1";

/// Client → scope message types.
pub mod c {
    pub const SPAWN: u8 = 0x01;
    pub const DESPAWN: u8 = 0x02;
    pub const TRANSFORM: u8 = 0x03;
    pub const STATE_SET: u8 = 0x04;
    pub const EVENT: u8 = 0x05;
    pub const INTEREST: u8 = 0x06;
    pub const PING: u8 = 0x07;
    pub const CLAIM: u8 = 0x08;
    pub const WORLD_MOD: u8 = 0x09;
    pub const CHUNKS: u8 = 0x0a;
    pub const ANIM: u8 = 0x0b;
    pub const WORLD_DESC: u8 = 0x0c;
}

/// Scope → client message types.
pub mod s {
    pub const WELCOME: u8 = 0x81;
    pub const ENTITY: u8 = 0x82;
    pub const GONE: u8 = 0x83;
    pub const TRANSFORM: u8 = 0x84;
    pub const STATE: u8 = 0x85;
    pub const STATE_REJECT: u8 = 0x86;
    pub const EVENT: u8 = 0x87;
    pub const PRESENCE: u8 = 0x88;
    pub const PONG: u8 = 0x89;
    pub const ERROR: u8 = 0x8a;
    pub const SPAWNED: u8 = 0x8b;
    pub const WORLD_MOD: u8 = 0x8c;
    pub const OWNER: u8 = 0x8d;
    pub const ANIM: u8 = 0x8e;
    pub const READY: u8 = 0x8f;
    pub const WORLD_DESC: u8 = 0x90;
    pub const CHUNK: u8 = 0x91;
}

/// Transform field mask bits.
pub mod f {
    pub const POS: u8 = 1;
    pub const ROT: u8 = 2;
    pub const SCALE: u8 = 4;
    pub const VEL: u8 = 8;
    pub const ANGVEL: u8 = 16;
}

/// "Write whatever the current revision is."
pub const ANY_REVISION: u32 = 0xffff_ffff;

/// Entity flags.
pub const ENTITY_PERSISTENT: u8 = 1;
pub const ENTITY_TRANSFERABLE: u8 = 2;
/// Spawn only if no entity of this kind exists; otherwise SPAWNED returns the
/// existing one (and its ENTITY follows), so shared scene objects exist once.
pub const ENTITY_UNIQUE: u8 = 4;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i32),
    Number(f64),
    Str(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolError(pub String);

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ProtocolError {}

type R<T> = Result<T, ProtocolError>;

// ------------------------------------------------------------------ writer

#[derive(Default, Clone, Debug)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self { buf: Vec::with_capacity(256) }
    }
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn f64(&mut self, v: f64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn raw(&mut self, b: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(b);
        self
    }
    pub fn str8(&mut self, s: &str) -> &mut Self {
        let b = truncate(s, 255);
        self.u8(b.len() as u8).raw(b)
    }
    pub fn str16(&mut self, s: &str) -> &mut Self {
        let b = truncate(s, 65535);
        self.u16(b.len() as u16).raw(b)
    }
    pub fn bytes16(&mut self, b: &[u8]) -> &mut Self {
        let b = &b[..b.len().min(65535)];
        self.u16(b.len() as u16).raw(b)
    }
    pub fn value(&mut self, v: &Value) -> &mut Self {
        match v {
            Value::Null => self.u8(0),
            Value::Bool(false) => self.u8(1),
            Value::Bool(true) => self.u8(2),
            Value::Number(n) => self.u8(3).f64(*n),
            Value::Int(i) => self.u8(4).i32(*i),
            Value::Str(s) => self.u8(5).str16(s),
            Value::Bytes(b) => self.u8(6).bytes16(b),
        }
    }
    pub fn transform(&mut self, t: &NetTransform) -> &mut Self {
        t.encode(self);
        self
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

/// UTF-8 bytes of `s`, cut at a character boundary to at most `max` bytes.
fn truncate(s: &str, max: usize) -> &[u8] {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s.as_bytes()[..end]
}

// ------------------------------------------------------------------ reader

pub struct Reader<'a> {
    bytes: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub fn done(&self) -> bool {
        self.pos >= self.bytes.len()
    }
    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        if self.pos + n > self.bytes.len() {
            return Err(ProtocolError("message ends early".into()));
        }
        let b = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(b)
    }
    pub fn u8(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> R<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn i16(&mut self) -> R<i16> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> R<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> R<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> R<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn f64(&mut self) -> R<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn bytes(&mut self, n: usize) -> R<&'a [u8]> {
        self.take(n)
    }
    pub fn str8(&mut self) -> R<String> {
        let n = self.u8()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| ProtocolError("bad utf-8".into()))
    }
    pub fn str16(&mut self) -> R<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| ProtocolError("bad utf-8".into()))
    }
    pub fn bytes16(&mut self) -> R<Vec<u8>> {
        let n = self.u16()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    pub fn value(&mut self) -> R<Value> {
        Ok(match self.u8()? {
            0 => Value::Null,
            1 => Value::Bool(false),
            2 => Value::Bool(true),
            3 => Value::Number(self.f64()?),
            4 => Value::Int(self.i32()?),
            5 => Value::Str(self.str16()?),
            6 => Value::Bytes(self.bytes16()?),
            t => return Err(ProtocolError(format!("unknown value type {t}"))),
        })
    }
    pub fn transform(&mut self) -> R<NetTransform> {
        NetTransform::decode(self)
    }
}

// ------------------------------------------------------------------ transforms

/// The fields of a transform that travel. `None` = not sent (unchanged or not synced).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetTransform {
    pub position: Option<Vec3>,
    pub rotation: Option<Quat>,
    pub scale: Option<Vec3>,
    pub velocity: Option<Vec3>,
    pub angular_velocity: Option<Vec3>,
}

impl NetTransform {
    pub fn mask(&self) -> u8 {
        (self.position.is_some() as u8 * f::POS)
            | (self.rotation.is_some() as u8 * f::ROT)
            | (self.scale.is_some() as u8 * f::SCALE)
            | (self.velocity.is_some() as u8 * f::VEL)
            | (self.angular_velocity.is_some() as u8 * f::ANGVEL)
    }

    pub fn encode(&self, w: &mut Writer) {
        w.u8(self.mask());
        if let Some(p) = self.position {
            w.f32(p.x).f32(p.y).f32(p.z);
        }
        if let Some(q) = self.rotation {
            w.u32(pack_quat(q));
        }
        if let Some(s) = self.scale {
            w.f32(s.x).f32(s.y).f32(s.z);
        }
        for v in [self.velocity, self.angular_velocity].into_iter().flatten() {
            for c in v.to_array() {
                w.i16((c * 100.0).round().clamp(-32767.0, 32767.0) as i16);
            }
        }
    }

    pub fn decode(r: &mut Reader) -> R<Self> {
        let mask = r.u8()?;
        if mask & !0x1f != 0 {
            return Err(ProtocolError("unknown transform fields".into()));
        }
        let mut t = Self::default();
        if mask & f::POS != 0 {
            t.position = Some(Vec3::new(r.f32()?, r.f32()?, r.f32()?));
        }
        if mask & f::ROT != 0 {
            t.rotation = Some(unpack_quat(r.u32()?));
        }
        if mask & f::SCALE != 0 {
            t.scale = Some(Vec3::new(r.f32()?, r.f32()?, r.f32()?));
        }
        let i16v =
            |r: &mut Reader| -> R<Vec3> { Ok(Vec3::new(r.i16()? as f32, r.i16()? as f32, r.i16()? as f32) / 100.0) };
        if mask & f::VEL != 0 {
            t.velocity = Some(i16v(r)?);
        }
        if mask & f::ANGVEL != 0 {
            t.angular_velocity = Some(i16v(r)?);
        }
        Ok(t)
    }

    /// Fold a newer partial update into this one.
    pub fn merge(&mut self, newer: &NetTransform) {
        self.position = newer.position.or(self.position);
        self.rotation = newer.rotation.or(self.rotation);
        self.scale = newer.scale.or(self.scale);
        self.velocity = newer.velocity.or(self.velocity);
        self.angular_velocity = newer.angular_velocity.or(self.angular_velocity);
    }
}

/// Smallest-three quaternion in 32 bits: 2 bits for the dropped (largest)
/// component, 10 bits for each of the other three in [-1/√2, 1/√2].
/// Error is under 0.0015 per component (about 0.1°).
pub fn pack_quat(q: Quat) -> u32 {
    let q = q.normalize();
    let a = q.to_array();
    let mut largest = 0;
    for i in 1..4 {
        if a[i].abs() > a[largest].abs() {
            largest = i;
        }
    }
    let sign = if a[largest] < 0.0 { -1.0 } else { 1.0 };
    const R: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let mut out = largest as u32;
    for (i, v) in a.iter().enumerate() {
        if i == largest {
            continue;
        }
        let n = ((v * sign + R) / (2.0 * R) * 1023.0).round().clamp(0.0, 1023.0) as u32;
        out = (out << 10) | n;
    }
    out
}

pub fn unpack_quat(bits: u32) -> Quat {
    const R: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let largest = (bits >> 30) as usize;
    let mut vals = [0f32; 3];
    for (k, v) in vals.iter_mut().enumerate() {
        let n = (bits >> (20 - 10 * k as u32)) & 1023;
        *v = n as f32 / 1023.0 * 2.0 * R - R;
    }
    let sum: f32 = vals.iter().map(|v| v * v).sum();
    let mut a = [0f32; 4];
    let mut j = 0;
    for (i, slot) in a.iter_mut().enumerate() {
        if i == largest {
            *slot = (1.0 - sum).max(0.0).sqrt();
        } else {
            *slot = vals[j];
            j += 1;
        }
    }
    Quat::from_array(a).normalize()
}

/// Animation: state id, normalized time and speed (5 bytes on the wire).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetAnim {
    pub state: u16,
    pub time: f32,
    pub speed: f32,
}

impl NetAnim {
    pub fn encode(&self, w: &mut Writer) {
        w.u16(self.state)
            .u16((self.time.clamp(0.0, 1.0) * 65535.0).round() as u16)
            .u8((self.speed * 16.0).round().clamp(0.0, 255.0) as u8);
    }
    pub fn decode(r: &mut Reader) -> R<Self> {
        Ok(Self { state: r.u16()?, time: r.u16()? as f32 / 65535.0, speed: r.u8()? as f32 / 16.0 })
    }
}

// ------------------------------------------------------------------ worlds

/// The seed a scene is generated from.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldDescriptor {
    pub id: String,
    pub seed: f64,
    pub generator: String,
    pub generator_version: String,
    pub chunk_size: u32,
    pub compatibility_hash: String,
}

impl WorldDescriptor {
    pub fn write(d: Option<&WorldDescriptor>, w: &mut Writer) {
        match d {
            None => {
                w.u8(0);
            }
            Some(d) => {
                w.u8(1)
                    .str8(&d.id)
                    .f64(d.seed)
                    .str8(&d.generator)
                    .str8(&d.generator_version)
                    .u32(d.chunk_size)
                    .str8(&d.compatibility_hash);
            }
        }
    }
    pub fn read(r: &mut Reader) -> R<Option<Self>> {
        if r.u8()? == 0 {
            return Ok(None);
        }
        Ok(Some(Self {
            id: r.str8()?,
            seed: r.f64()?,
            generator: r.str8()?,
            generator_version: r.str8()?,
            chunk_size: r.u32()?,
            compatibility_hash: r.str8()?,
        }))
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |h, b| (h ^ *b as u64).wrapping_mul(FNV_PRIME))
}

/// One modification's share of the world hash (content-defined, order-free).
pub fn mod_hash(cx: i16, cz: i16, key: &str, v: &Value) -> u64 {
    let mut w = Writer::new();
    w.i16(cx).i16(cz).raw(key.as_bytes()).u8(0).value(v);
    fnv1a64(&w.buf)
}

/// The initial seed's share of the world hash.
pub fn seed_hash(d: Option<&WorldDescriptor>) -> u64 {
    let Some(d) = d else { return 0 };
    let mut w = Writer::new();
    w.str8(&d.id).f64(d.seed).str8(&d.generator).str8(&d.generator_version).u32(d.chunk_size);
    fnv1a64(&w.buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_the_worker() {
        // Golden values shared with kit-ai's test/circuits.test.ts.
        let removed = Value::Str("removed".into());
        assert_eq!(mod_hash(10, 12, "tree:42", &removed), 0xa898_0eaa_b5bc_d69b);
        assert_eq!(mod_hash(-3, 7, "rock:1", &Value::Int(5)), 0xe2be_a47b_379b_2d3f);
        let d = WorldDescriptor {
            id: "forest".into(),
            seed: 847291.0,
            generator: "kit.terrain".into(),
            generator_version: "1".into(),
            chunk_size: 32,
            compatibility_hash: String::new(),
        };
        assert_eq!(seed_hash(Some(&d)), 0x4090_1844_b0a6_2917);
        assert_eq!(seed_hash(Some(&d)).wrapping_add(mod_hash(10, 12, "tree:42", &removed)), 0xe928_26ef_6662_ffb2);
    }

    #[test]
    fn quaternions_survive_32_bits() {
        for (axis, angle) in
            [(Vec3::Y, 0.0), (Vec3::Y, 1.0), (Vec3::new(1.0, 2.0, -0.5).normalize(), 2.9), (Vec3::X, -3.1)]
        {
            let q = Quat::from_axis_angle(axis, angle);
            let back = unpack_quat(pack_quat(q));
            assert!(q.dot(back).abs() > 0.99999, "{q:?} → {back:?}");
        }
    }

    #[test]
    fn transforms_round_trip_and_merge() {
        let t = NetTransform {
            position: Some(Vec3::new(1.5, -2.0, 3.25)),
            rotation: Some(Quat::from_rotation_y(0.7)),
            velocity: Some(Vec3::new(4.21, 0.0, -1.0)),
            ..Default::default()
        };
        let mut w = Writer::new();
        t.encode(&mut w);
        assert_eq!(w.buf.len(), 1 + 12 + 4 + 6, "pos + rot + vel = 23 bytes");
        let back = Reader::new(&w.buf).transform().unwrap();
        assert_eq!(back.position, t.position);
        assert!(back.velocity.unwrap().abs_diff_eq(t.velocity.unwrap(), 0.01));
        let mut merged = back;
        merged.merge(&NetTransform { position: Some(Vec3::ZERO), ..Default::default() });
        assert_eq!(merged.position, Some(Vec3::ZERO));
        assert!(merged.rotation.is_some());
        assert!(Reader::new(&[0x40]).transform().is_err(), "unknown bits are refused");
        assert!(Reader::new(&[f::POS, 0, 0]).transform().is_err(), "short frames are refused");
    }

    #[test]
    fn values_round_trip() {
        for v in [
            Value::Null,
            Value::Bool(true),
            Value::Int(-7),
            Value::Number(0.25),
            Value::Str("rain ☔".into()),
            Value::Bytes(vec![1, 2, 3]),
        ] {
            let mut w = Writer::new();
            w.value(&v);
            assert_eq!(Reader::new(&w.buf).value().unwrap(), v);
        }
    }
}
