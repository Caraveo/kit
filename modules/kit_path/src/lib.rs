//! # kit_path
//!
//! Makes entities follow paths sketched in the editor's **Canvas mode**
//! ([`PathFollow`]). Following is best effort and respects the world:
//!
//! * **Dynamic rigid bodies** are steered by velocity. Physics resolves the
//!   motion, so walls stop them, they push other bodies, and gravity still
//!   applies. A falling body only steers horizontally.
//! * **Colliders** (kinematic or collider-only) look ahead with a ray cast and
//!   slide along obstacles instead of passing through them.
//! * **Plain entities** move straight along the path.
//!
//! When an entity makes no progress toward a point for a while (stuck behind
//! something), it skips to the next point.

use glam::{Mat4, Quat, Vec3};
use kit_core::{App, Module, Stage, Time};
use kit_ecs::{Entity, World};
use kit_gamesys::{BodyKind, Collider, ColliderShape, PathFollow, PathLoop, Physics, RigidBody};
use kit_scene::GlobalTransform;

/// Seconds without progress before skipping a point.
const STUCK_SECONDS: f32 = 1.2;

/// Runtime progress along a path (not saved).
#[derive(Clone, Debug)]
pub struct PathState {
    pub index: usize,
    /// +1 forward, -1 backward (ping-pong).
    pub direction: i32,
    pub finished: bool,
    /// Physics body handed back to the simulation (gravity restored).
    released: bool,
    best_distance: f32,
    stuck: f32,
    points_hash: u64,
}

fn hash_points(points: &[Vec3]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in points {
        for v in p.to_array() {
            h = (h ^ v.to_bits() as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

impl PathState {
    fn new(path: &PathFollow, pos: Vec3) -> Self {
        // Skip the first point when the entity already stands on it.
        let index = if path.points.len() > 1 && path.points[0].distance(pos) < 0.25 { 1 } else { 0 };
        Self {
            index,
            direction: 1,
            finished: false,
            released: false,
            best_distance: f32::MAX,
            stuck: 0.0,
            points_hash: hash_points(&path.points),
        }
    }

    /// Move to the next point; returns false when the path is over.
    fn advance(&mut self, len: usize, mode: PathLoop) -> bool {
        self.best_distance = f32::MAX;
        self.stuck = 0.0;
        let next = self.index as i64 + self.direction as i64;
        if next >= 0 && (next as usize) < len {
            self.index = next as usize;
            return true;
        }
        match mode {
            PathLoop::Once => {
                self.finished = true;
                false
            }
            PathLoop::Loop => {
                self.index = 0;
                true
            }
            PathLoop::PingPong => {
                self.direction = -self.direction;
                self.index = (self.index as i64 + self.direction as i64).clamp(0, len as i64 - 1) as usize;
                len > 1
            }
        }
    }
}

/// Rough radius of an entity's collider, for look-ahead.
fn collider_radius(c: &Collider, scale: Vec3) -> f32 {
    let s = scale.abs();
    match c.shape {
        ColliderShape::Box { size } => (size * s * 0.5).max_element(),
        ColliderShape::Sphere { radius } => radius * s.max_element(),
        ColliderShape::Capsule { radius, .. } | ColliderShape::Cylinder { radius, .. } => radius * s.x.max(s.z),
    }
}

enum Mover {
    /// Steered by setting velocity on a dynamic body.
    Dynamic { ground: bool },
    /// Moved by us, sliding around obstacles.
    Collide { radius: f32 },
    /// Moved directly.
    Free,
}

fn set_world_position(world: &mut World, e: Entity, pos: Vec3, rot: Option<Quat>) {
    let Some(m) = world.get::<GlobalTransform>(e).map(|g| g.0) else { return };
    let (s, r, _) = m.to_scale_rotation_translation();
    kit_scene::set_world_matrix(world, e, Mat4::from_scale_rotation_translation(s, rot.unwrap_or(r), pos));
    if let Some(mut g) = world.get_mut::<GlobalTransform>(e) {
        g.0 = Mat4::from_scale_rotation_translation(s, rot.unwrap_or(r), pos);
    }
}

/// The per-frame path following system (play mode).
pub fn follow_paths(world: &mut World) {
    let dt = world.resource::<Time>().delta;
    if dt <= 0.0 {
        return;
    }
    kit_scene::propagate_transforms(world);
    for e in world.query::<(PathFollow,)>() {
        let mut path = world.get_cloned::<PathFollow>(e).unwrap();
        if path.points.is_empty() {
            continue;
        }
        // Follow the smoothed curve (or the raw points).
        path.points = path.curve();
        let Some(m) = world.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
        let (scale, rot, pos) = m.to_scale_rotation_translation();

        let mut state = match world.get_cloned::<PathState>(e) {
            Some(s) if s.points_hash == hash_points(&path.points) => s,
            // Points changed (e.g. extended while playing): keep going from the nearest point.
            _ => PathState::new(&path, pos),
        };
        let has_body = world.try_resource::<Physics>().is_some_and(|p| p.0.linear_velocity(e).is_some());
        let body = world.get_cloned::<RigidBody>(e);
        let mover = match (&body, has_body) {
            // "Fly": while following, gravity is suspended and the body is steered in 3D.
            (Some(b), true) if b.kind == BodyKind::Dynamic => Mover::Dynamic { ground: false },
            _ => match world.get::<Collider>(e) {
                Some(c) if world.has_resource::<Physics>() && !c.trigger => {
                    Mover::Collide { radius: collider_radius(&c, scale) }
                }
                _ => Mover::Free,
            },
        };

        if !path.playing || state.finished {
            if let (Mover::Dynamic { .. }, Some(mut phys)) = (&mover, world.try_resource_mut::<Physics>()) {
                // Hand the body back to physics: gravity on, sideways motion stopped once.
                if !state.released {
                    phys.0.suspend_gravity(e, false);
                    let v = phys.0.linear_velocity(e).unwrap_or_default();
                    phys.0.set_linear_velocity(e, Vec3::new(0.0, v.y.min(0.0), 0.0));
                }
            }
            state.released = true;
            world.insert(e, state);
            continue;
        }
        if let (Mover::Dynamic { .. }, Some(mut phys)) = (&mover, world.try_resource_mut::<Physics>()) {
            phys.0.suspend_gravity(e, true);
        }
        state.released = false;

        let len = path.points.len();
        // Planar when the body is held down by gravity: only steer in X/Z (X in 2D).
        let planar = matches!(mover, Mover::Dynamic { ground: true });
        let flatten = |v: Vec3| if planar { Vec3::new(v.x, 0.0, v.z) } else { v };
        let mut target = path.points[state.index.min(len - 1)];
        let mut to = flatten(target - pos);
        // Entities we move land exactly on points; physics bodies need some slack.
        let arrive = if matches!(mover, Mover::Dynamic { .. }) { (path.speed * dt * 1.5).max(0.08) } else { 1e-3 };
        // Consume points we are already at (several per frame for dense sketches).
        let mut guard = 0;
        while to.length() <= arrive && guard < len + 1 {
            guard += 1;
            if !state.advance(len, path.mode) {
                break;
            }
            target = path.points[state.index];
            to = flatten(target - pos);
        }
        if state.finished {
            world.insert(e, state);
            continue;
        }
        let dist = to.length();
        let dir = to.normalize_or_zero();

        // Stuck detection: no progress for a while -> skip this point.
        if dist < state.best_distance - 0.02 {
            state.best_distance = dist;
            state.stuck = 0.0;
        } else {
            state.stuck += dt;
            if state.stuck > STUCK_SECONDS {
                state.advance(len, path.mode);
            }
        }

        let facing = (path.face_direction && Vec3::new(dir.x, 0.0, dir.z).length() > 0.3)
            .then(|| Quat::from_rotation_y(f32::atan2(-dir.x, -dir.z)));
        // Turn smoothly toward the direction of travel.
        let new_rot = facing.map(|f| rot.slerp(f, (dt * 10.0).min(1.0)));

        match mover {
            Mover::Dynamic { ground } => {
                if let Some(mut phys) = world.try_resource_mut::<Physics>() {
                    let current = phys.0.linear_velocity(e).unwrap_or_default();
                    let speed = path.speed.min(dist / dt);
                    let mut v = dir * speed;
                    if ground {
                        v.y = current.y;
                    }
                    phys.0.set_linear_velocity(e, v);
                }
            }
            Mover::Collide { radius } => {
                let step_len = (path.speed * dt).min(dist);
                let mut step = dir * step_len;
                if let Some(phys) = world.try_resource::<Physics>() {
                    // Look ahead; slide along whatever is in the way.
                    for _ in 0..2 {
                        let len = step.length();
                        if len < 1e-6 {
                            break;
                        }
                        match phys.0.raycast(pos, step / len, len + radius, Some(e)) {
                            Some(hit) => {
                                let allowed = (hit.distance - radius - 0.01).max(0.0);
                                let n = hit.normal;
                                let into = step.dot(n).min(0.0);
                                let slide = step - n * into;
                                step = if allowed > 1e-3 { step / len * allowed.min(len) } else { slide };
                                if allowed > 1e-3 {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
                set_world_position(world, e, pos + step, new_rot);
            }
            Mover::Free => {
                let step = dir * (path.speed * dt).min(dist);
                set_world_position(world, e, pos + step, new_rot);
            }
        }
        world.insert(e, state);
    }
}

/// Forget all path progress (play stopped).
pub fn reset(world: &mut World) {
    for e in world.query::<(PathState,)>() {
        world.remove::<PathState>(e);
    }
}

/// Follows sketched paths. Add after the script module and before the collider module.
pub struct PathModule;

impl Module for PathModule {
    fn name(&self) -> &'static str {
        "path"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["scene", "gamesys"]
    }
    fn build(&self, app: &mut App) {
        app.add_play_system(Stage::Update, "follow_paths", follow_paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kit_gamesys::GameSysModule;
    use kit_scene::{SceneModule, Transform};

    fn app(physics: bool) -> App {
        let mut app = App::new();
        app.add_module(SceneModule).add_module(GameSysModule).add_module(PathModule);
        if physics {
            app.add_module(kit_collider::ColliderModule).add_module(kit_physics::PhysicsModule::default());
        }
        app
    }

    fn pos(app: &App, e: Entity) -> Vec3 {
        app.world.get::<Transform>(e).unwrap().translation
    }

    #[test]
    fn free_entity_walks_the_path() {
        let mut app = app(false);
        let e = kit_scene::spawn_node(&mut app.world, "Walker", Transform::IDENTITY);
        let points = vec![Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 2.0)];
        app.world.insert(e, PathFollow { points, speed: 2.0, smooth: false, ..Default::default() });
        for _ in 0..60 {
            app.update(1.0 / 60.0);
        }
        // 2 units/s for 1s: reached the corner.
        assert!(pos(&app, e).distance(Vec3::new(2.0, 0.0, 0.0)) < 0.1, "{}", pos(&app, e));
        for _ in 0..90 {
            app.update(1.0 / 60.0);
        }
        assert!(pos(&app, e).distance(Vec3::new(2.0, 0.0, 2.0)) < 0.05, "{}", pos(&app, e));
        assert!(app.world.get::<PathState>(e).unwrap().finished);
    }

    #[test]
    fn ping_pong_turns_around() {
        let mut app = app(false);
        let e = kit_scene::spawn_node(&mut app.world, "P", Transform::IDENTITY);
        let points = vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)];
        app.world.insert(
            e,
            PathFollow { points, speed: 2.0, mode: PathLoop::PingPong, smooth: false, ..Default::default() },
        );
        for _ in 0..45 {
            app.update(1.0 / 60.0);
        }
        // Went out to x = 1 (0.5s) and is coming back.
        let x = pos(&app, e).x;
        assert!(x < 0.9 && x > 0.3, "x = {x}");
    }

    #[test]
    fn collider_slides_and_does_not_pass_through_walls() {
        let mut app = app(true);
        let w = &mut app.world;
        let wall =
            kit_scene::spawn_node(w, "Wall", Transform::from_xyz(3.0, 0.0, 0.0).with_scale(Vec3::new(0.5, 3.0, 3.0)));
        w.insert(wall, Collider::default());
        let e = kit_scene::spawn_node(w, "Mover", Transform::IDENTITY);
        w.insert(e, Collider { shape: ColliderShape::Sphere { radius: 0.5 }, ..Default::default() });
        w.insert(
            e,
            PathFollow { points: vec![Vec3::ZERO, Vec3::new(6.0, 0.0, 0.0)], speed: 4.0, ..Default::default() },
        );
        for _ in 0..90 {
            app.update(1.0 / 60.0);
            // Never inside the wall (wall face at x = 2.75, mover radius 0.5).
            assert!(pos(&app, e).x <= 2.26, "passed into the wall: {}", pos(&app, e));
        }
    }

    #[test]
    fn dynamic_body_flies_the_path_then_falls() {
        let mut app = app(true);
        let w = &mut app.world;
        let ground = kit_scene::spawn_node(
            w,
            "Ground",
            Transform::from_xyz(0.0, -0.5, 0.0).with_scale(Vec3::new(40.0, 1.0, 40.0)),
        );
        w.insert(ground, Collider::default());
        w.insert(ground, RigidBody { kind: BodyKind::Static, ..Default::default() });
        let e = kit_scene::spawn_node(w, "Ball", Transform::from_xyz(0.0, 0.5, 0.0));
        w.insert(e, Collider { shape: ColliderShape::Sphere { radius: 0.5 }, ..Default::default() });
        w.insert(e, RigidBody { lock_rotation: true, ..Default::default() });
        // A climb into the air: the body flies it in 3D (gravity suspended).
        let points = vec![Vec3::new(0.0, 0.5, 0.0), Vec3::new(0.0, 3.0, -3.0), Vec3::new(3.0, 3.0, -3.0)];
        w.insert(e, PathFollow { points, speed: 3.0, smooth: false, ..Default::default() });
        let mut peak = 0.0f32;
        for _ in 0..150 {
            app.update(1.0 / 60.0);
            peak = peak.max(pos(&app, e).y);
        }
        let p = pos(&app, e);
        assert!(peak > 2.7, "flew up the path: peak {peak}");
        assert!(Vec3::new(p.x, 0.0, p.z).distance(Vec3::new(3.0, 0.0, -3.0)) < 0.4, "{p}");
        // Path over: gravity is back and it lands.
        for _ in 0..180 {
            app.update(1.0 / 60.0);
        }
        assert!((pos(&app, e).y - 0.5).abs() < 0.1, "fell back to the ground: {}", pos(&app, e));
    }
}
