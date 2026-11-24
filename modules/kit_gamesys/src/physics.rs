//! Physics components and the engine-neutral physics API.
//!
//! The data lives here (so scenes can save it and scripts can use it); the
//! simulation lives in the `kit_collider` and `kit_physics` modules.

use glam::Vec3;
use kit_ecs::Entity;
use serde::{Deserialize, Serialize};
use std::any::Any;

/// Collision shape, in the entity's local space (scaled by its world scale).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ColliderShape {
    Box {
        size: Vec3,
    },
    Sphere {
        radius: f32,
    },
    /// Upright (Y axis) capsule; `height` is the full height including the caps.
    Capsule {
        height: f32,
        radius: f32,
    },
    /// Upright (Y axis) cylinder.
    Cylinder {
        height: f32,
        radius: f32,
    },
}

impl ColliderShape {
    pub fn label(&self) -> &'static str {
        match self {
            ColliderShape::Box { .. } => "Box",
            ColliderShape::Sphere { .. } => "Sphere",
            ColliderShape::Capsule { .. } => "Capsule",
            ColliderShape::Cylinder { .. } => "Cylinder",
        }
    }

    /// The shape that best fits a built-in mesh.
    pub fn fit(mesh: crate::MeshKind) -> Self {
        use crate::MeshKind::*;
        match mesh {
            Cube => ColliderShape::Box { size: Vec3::ONE },
            Sphere => ColliderShape::Sphere { radius: 0.5 },
            Plane => ColliderShape::Box { size: Vec3::new(1.0, 0.02, 1.0) },
            Quad => ColliderShape::Box { size: Vec3::new(1.0, 1.0, 0.02) },
            Cylinder => ColliderShape::Cylinder { height: 1.0, radius: 0.5 },
            Torus => ColliderShape::Cylinder { height: 0.3, radius: 0.5 },
        }
    }
}

/// A collision shape. Without a [`RigidBody`] it is a static collider that
/// follows its transform (good for level geometry and triggers).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Collider {
    pub shape: ColliderShape,
    /// Local offset of the shape from the entity origin.
    pub offset: Vec3,
    /// A trigger reports overlaps but does not block anything.
    pub trigger: bool,
    pub friction: f32,
    /// Bounciness, 0..1.
    pub restitution: f32,
}

impl Default for Collider {
    fn default() -> Self {
        Self {
            shape: ColliderShape::Box { size: Vec3::ONE },
            offset: Vec3::ZERO,
            trigger: false,
            friction: 0.6,
            restitution: 0.1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyKind {
    /// Moved by the simulation (gravity, collisions, forces).
    #[default]
    Dynamic,
    /// Moved by your scripts; pushes dynamic bodies out of the way.
    Kinematic,
    /// Never moves.
    Static,
}

impl BodyKind {
    pub const ALL: [BodyKind; 3] = [BodyKind::Dynamic, BodyKind::Kinematic, BodyKind::Static];
    pub fn label(self) -> &'static str {
        match self {
            BodyKind::Dynamic => "Dynamic",
            BodyKind::Kinematic => "Kinematic",
            BodyKind::Static => "Static",
        }
    }
}

/// Makes the entity part of the physics simulation. Needs a [`Collider`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RigidBody {
    pub kind: BodyKind,
    /// Mass in kg (dynamic bodies).
    pub mass: f32,
    pub gravity_scale: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    /// Keep the body upright (characters).
    pub lock_rotation: bool,
    /// Constrain to the XY plane for 2D games.
    pub lock_2d: bool,
    /// Continuous collision detection for fast objects.
    pub ccd: bool,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self {
            kind: BodyKind::Dynamic,
            mass: 1.0,
            gravity_scale: 1.0,
            linear_damping: 0.0,
            angular_damping: 0.05,
            lock_rotation: false,
            lock_2d: false,
            ccd: false,
        }
    }
}

/// One collision this frame between two entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollisionEvent {
    pub a: Entity,
    pub b: Entity,
    /// `true` when contact began, `false` when it ended.
    pub started: bool,
    /// One of the two colliders is a trigger.
    pub trigger: bool,
}

/// Collision events produced by the last physics step (a world resource).
#[derive(Default, Debug)]
pub struct CollisionEvents {
    pub events: Vec<CollisionEvent>,
}

/// Result of a ray cast.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub entity: Entity,
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
}

/// What scripts (and Rust code) can ask of the physics backend.
/// Implemented by `kit_collider`; reach it through the [`Physics`] resource.
pub trait PhysicsApi: Any {
    fn linear_velocity(&self, e: Entity) -> Option<Vec3>;
    fn set_linear_velocity(&mut self, e: Entity, v: Vec3);
    fn angular_velocity(&self, e: Entity) -> Option<Vec3>;
    fn set_angular_velocity(&mut self, e: Entity, v: Vec3);
    fn apply_impulse(&mut self, e: Entity, impulse: Vec3);
    /// Continuous force, applied during the next step only.
    fn add_force(&mut self, e: Entity, force: Vec3);
    fn raycast(&self, origin: Vec3, dir: Vec3, max_distance: f32, exclude: Option<Entity>) -> Option<RayHit>;
    /// Entities currently touching `e`.
    fn contacts(&self, e: Entity) -> Vec<Entity>;
    /// Temporarily ignore gravity for one body (path following in "fly" mode).
    fn suspend_gravity(&mut self, e: Entity, suspended: bool);
    fn gravity(&self) -> Vec3;
    fn set_gravity(&mut self, g: Vec3);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// The physics backend, as a world resource.
pub struct Physics(pub Box<dyn PhysicsApi>);

/// What a path follower does at the end of its path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathLoop {
    /// Stop at the last point.
    #[default]
    Once,
    /// Jump back to the first point and go again.
    Loop,
    /// Walk the path back and forth.
    PingPong,
}

impl PathLoop {
    pub const ALL: [PathLoop; 3] = [PathLoop::Once, PathLoop::Loop, PathLoop::PingPong];
    pub fn label(self) -> &'static str {
        match self {
            PathLoop::Once => "Once",
            PathLoop::Loop => "Loop",
            PathLoop::PingPong => "Ping-pong",
        }
    }
}

/// A movement path sketched in the editor's Canvas mode. While playing, the
/// entity follows it as well as it can: dynamic bodies are steered by velocity
/// (so colliders block them and they push things), colliders slide around
/// obstacles, and plain entities move directly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PathFollow {
    /// World-space points.
    pub points: Vec<Vec3>,
    /// Units per second.
    pub speed: f32,
    pub mode: PathLoop,
    /// Start following when play starts (scripts can use `Path.Play()`).
    pub playing: bool,
    /// Turn to face the direction of travel.
    pub face_direction: bool,
    /// Follow a smooth curve through the points instead of straight lines.
    pub smooth: bool,
}

impl Default for PathFollow {
    fn default() -> Self {
        Self { points: Vec::new(), speed: 3.0, mode: PathLoop::Once, playing: true, face_direction: true, smooth: true }
    }
}

impl PathFollow {
    /// The line actually followed: the points, or a smooth curve through them.
    pub fn curve(&self) -> Vec<Vec3> {
        if !self.smooth || self.points.len() < 3 {
            return self.points.clone();
        }
        let p = &self.points;
        let n = p.len();
        let closed = self.mode == PathLoop::Loop && p[0].distance(p[n - 1]) < 1e-3;
        let get = |i: isize| -> Vec3 {
            if closed {
                // Skip the duplicated end point when wrapping around.
                let m = (n - 1) as isize;
                p[(((i % m) + m) % m) as usize]
            } else {
                p[i.clamp(0, n as isize - 1) as usize]
            }
        };
        let mut out = vec![p[0]];
        for i in 0..(n - 1) as isize {
            let (p0, p1, p2, p3) = (get(i - 1), get(i), get(i + 1), get(i + 2));
            let len = p1.distance(p2);
            let steps = ((len / 0.25).ceil() as usize).clamp(2, 32);
            for s in 1..=steps {
                let t = s as f32 / steps as f32;
                let (t2, t3) = (t * t, t * t * t);
                // Catmull-Rom: passes through every point.
                let v = 0.5
                    * ((2.0 * p1)
                        + (-p0 + p2) * t
                        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
                out.push(v);
            }
        }
        out
    }
}

#[cfg(test)]
mod curve_tests {
    use super::*;

    #[test]
    fn smooth_curve_passes_through_points() {
        let pts = vec![Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 1.0)];
        let p = PathFollow { points: pts.clone(), ..Default::default() };
        let c = p.curve();
        assert!(c.len() > pts.len() * 3);
        for q in &pts {
            assert!(c.iter().any(|v| v.distance(*q) < 1e-4), "curve passes through {q}");
        }
        assert_eq!(PathFollow { points: pts.clone(), smooth: false, ..Default::default() }.curve(), pts);
    }
}
