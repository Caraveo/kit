//! Rigg: volume rigs. Primitive volumes (spheres, capsules, boxes) placed
//! inside a model deform its mesh; there are no bones. The data lives here
//! (so scenes save and load it); `kit_rigg` binds, deforms and animates.

use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

/// The shape of a rig volume, in the volume object's own space (centered on
/// its origin; capsules run along local Y).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape")]
pub enum RigShape {
    Sphere {
        radius: f32,
    },
    /// `length` is the distance between the two cap centers.
    Capsule {
        radius: f32,
        length: f32,
    },
    /// Full size along X, Y, Z.
    Box {
        size: Vec3,
    },
}

impl Default for RigShape {
    fn default() -> Self {
        RigShape::Sphere { radius: 0.1 }
    }
}

impl RigShape {
    pub fn label(&self) -> &'static str {
        match self {
            RigShape::Sphere { .. } => "Sphere",
            RigShape::Capsule { .. } => "Capsule",
            RigShape::Box { .. } => "Box",
        }
    }

    /// Signed distance from `p` (volume space) to the surface; negative inside.
    pub fn distance(&self, p: Vec3) -> f32 {
        match *self {
            RigShape::Sphere { radius } => p.length() - radius,
            RigShape::Capsule { radius, length } => {
                let h = length * 0.5;
                let y = p.y.clamp(-h, h);
                (p - Vec3::new(0.0, y, 0.0)).length() - radius
            }
            RigShape::Box { size } => {
                let q = p.abs() - size * 0.5;
                q.max(Vec3::ZERO).length() + q.x.max(q.y.max(q.z)).min(0.0)
            }
        }
    }

    /// Rough size, used to scale how strongly "deep inside" counts.
    pub fn extent(&self) -> f32 {
        match *self {
            RigShape::Sphere { radius } => radius,
            RigShape::Capsule { radius, .. } => radius,
            RigShape::Box { size } => size.min_element() * 0.5,
        }
        .max(1e-3)
    }
}

/// One rig control: a primitive volume on a child object of the rigged model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigVolume {
    pub shape: RigShape,
    /// How strongly it pulls the mesh (0–1).
    pub influence: f32,
    /// How far beyond its surface it still has some pull (in world units).
    pub falloff: f32,
    /// Its transform relative to the rig at bind time (the rest pose). `None`
    /// until the rig is bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest: Option<Mat4>,
    /// Extra connections shown in Rig mode (names of other volumes).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
}

impl Default for RigVolume {
    fn default() -> Self {
        Self { shape: RigShape::default(), influence: 1.0, falloff: 0.15, rest: None, links: Vec::new() }
    }
}

/// A volume's pose: its local transform (relative to its parent).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RigKey {
    pub time: f32,
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

/// The keys for one volume (by its name under the rig).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RigTrack {
    pub volume: String,
    pub keys: Vec<RigKey>,
}

/// An animation clip: keyed volume poses over time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigClip {
    pub name: String,
    /// Seconds.
    pub length: f32,
    pub looping: bool,
    pub tracks: Vec<RigTrack>,
}

impl Default for RigClip {
    fn default() -> Self {
        Self { name: "Idle".into(), length: 2.0, looping: true, tracks: Vec::new() }
    }
}

/// A saved pose: every volume's local transform, by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RigPose {
    pub name: String,
    pub volumes: Vec<(String, [f32; 10])>,
}

/// A volume rig on a model. Its volumes are descendants with [`RigVolume`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rig {
    pub enabled: bool,
    /// Dual-quaternion blending: keeps limbs from thinning at twists and bends.
    pub preserve_volume: bool,
    /// Animation clips (the timeline edits these).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<RigClip>,
    /// The clip to play when the game starts ("" = none).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub autoplay: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub poses: Vec<RigPose>,
}

impl Default for Rig {
    fn default() -> Self {
        Self { enabled: true, preserve_volume: true, clips: Vec::new(), autoplay: String::new(), poses: Vec::new() }
    }
}

/// A transform as 10 floats (translation, rotation xyzw, scale) for poses.
pub fn pack_transform(t: &kit_scene::Transform) -> [f32; 10] {
    let (p, r, s) = (t.translation, t.rotation, t.scale);
    [p.x, p.y, p.z, r.x, r.y, r.z, r.w, s.x, s.y, s.z]
}

pub fn unpack_transform(v: &[f32; 10]) -> kit_scene::Transform {
    kit_scene::Transform {
        translation: Vec3::new(v[0], v[1], v[2]),
        rotation: Quat::from_xyzw(v[3], v[4], v[5], v[6]).normalize(),
        scale: Vec3::new(v[7], v[8], v[9]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_distances() {
        let s = RigShape::Sphere { radius: 1.0 };
        assert!((s.distance(Vec3::new(2.0, 0.0, 0.0)) - 1.0).abs() < 1e-6);
        assert!(s.distance(Vec3::ZERO) < 0.0);
        let c = RigShape::Capsule { radius: 0.5, length: 2.0 };
        assert!((c.distance(Vec3::new(0.0, 1.0, 1.0)) - 0.5).abs() < 1e-6, "beside the top cap center");
        assert!((c.distance(Vec3::new(0.0, 2.0, 0.0)) - 0.5).abs() < 1e-6, "past the end");
        let b = RigShape::Box { size: Vec3::splat(2.0) };
        assert!((b.distance(Vec3::new(3.0, 0.0, 0.0)) - 2.0).abs() < 1e-6);
        assert!((b.distance(Vec3::ZERO) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn rigs_round_trip_through_json() {
        let r = Rig {
            clips: vec![RigClip {
                tracks: vec![RigTrack {
                    volume: "Arm".into(),
                    keys: vec![RigKey { time: 0.5, translation: Vec3::X, rotation: Quat::IDENTITY, scale: Vec3::ONE }],
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let back: Rig = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
        let v = RigVolume { rest: Some(Mat4::from_translation(Vec3::Y)), ..Default::default() };
        let back: RigVolume = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(back, v);
    }
}
