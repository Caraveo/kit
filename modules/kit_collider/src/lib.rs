//! # kit_collider
//!
//! Collision detection for Kit, built on [Rapier](https://rapier.rs) (the role of
//! Defold's collision objects).
//!
//! * Every entity with a [`Collider`] gets a shape in the collision world.
//! * Without the physics module, colliders simply follow their transforms. Kit
//!   reports when they touch or overlap ([`CollisionEvents`], the scripts'
//!   `OnCollision` / `OnTrigger` hooks) and answers ray casts.
//! * The `kit_physics` module turns on dynamics: [`RigidBody`] entities fall,
//!   bounce and are pushed around, and Kit writes their poses back to `Transform`.
//!
//! The simulation runs at a fixed 60 Hz step, after scripts, in play mode only.

use glam::{Mat4, Quat, Vec3};
use kit_core::{App, Module, Stage, Time};
use kit_ecs::{Entity, World};
use kit_gamesys::{
    BodyKind, Collider, ColliderShape, CollisionEvent, CollisionEvents, Physics, PhysicsApi, RayHit, RigidBody, Terrain,
};
use kit_scene::GlobalTransform;
use rapier3d::math::{Pose as RPose, Rotation as RRot, Vector as RVec};
use rapier3d::pipeline::PhysicsWorld;
use rapier3d::prelude::{
    ActiveCollisionTypes, ActiveEvents, ChannelEventCollector, ColliderBuilder, ColliderHandle, CollisionEventFlags,
    LockedAxes, QueryFilter, Ray, RigidBodyBuilder, RigidBodyHandle, RigidBodyType,
};
use std::any::Any;
use std::collections::{HashMap, HashSet};

// glam (Kit) <-> glam (Rapier's version) conversions.
fn rv(v: Vec3) -> RVec {
    RVec::new(v.x, v.y, v.z)
}
fn gv(v: RVec) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}
fn rq(q: Quat) -> RRot {
    RRot::from_xyzw(q.x, q.y, q.z, q.w)
}
fn gq(q: RRot) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}
fn rpose(t: Vec3, r: Quat) -> RPose {
    RPose::from_parts(rv(t), rq(r))
}

enum Pending {
    LinVel(Vec3),
    AngVel(Vec3),
    Impulse(Vec3),
    Force(Vec3),
}

struct Entry {
    body: RigidBodyHandle,
    collider: ColliderHandle,
    collider_cfg: Collider,
    body_cfg: Option<RigidBody>,
    scale: Vec3,
    /// Fingerprint of the terrain this heightfield was built from.
    terrain: Option<u64>,
    dynamic: bool,
    /// Pose last written to (or read from) the entity, to spot script teleports.
    last: (Vec3, Quat),
}

/// The collision world (a [`PhysicsApi`] implementation).
pub struct CollisionWorld {
    pub world: PhysicsWorld,
    entries: HashMap<Entity, Entry>,
    by_collider: HashMap<ColliderHandle, Entity>,
    /// Simulate rigid bodies (set by `kit_physics`).
    pub dynamics: bool,
    gravity: Vec3,
    accumulator: f32,
    pub fixed_dt: f32,
    contacts: HashSet<(Entity, Entity)>,
    pending: Vec<(Entity, Pending)>,
    forced: HashSet<Entity>,
}

impl Default for CollisionWorld {
    fn default() -> Self {
        Self::new()
    }
}

fn pair(a: Entity, b: Entity) -> (Entity, Entity) {
    if a < b { (a, b) } else { (b, a) }
}

fn decompose(m: Mat4) -> (Vec3, Quat, Vec3) {
    let (s, r, t) = m.to_scale_rotation_translation();
    (s, r.normalize(), t)
}

fn close(a: (Vec3, Quat), b: (Vec3, Quat)) -> bool {
    a.0.abs_diff_eq(b.0, 1e-4) && a.1.dot(b.1).abs() > 1.0 - 1e-6
}

fn build_collider(c: &Collider, scale: Vec3, body: Option<&RigidBody>) -> rapier3d::prelude::Collider {
    let s = scale.abs().max(Vec3::splat(1e-4));
    let radial = s.x.max(s.z);
    let builder = match c.shape {
        ColliderShape::Box { size } => {
            let h = (size * s * 0.5).max(Vec3::splat(1e-3));
            ColliderBuilder::cuboid(h.x, h.y, h.z)
        }
        ColliderShape::Sphere { radius } => ColliderBuilder::ball((radius * s.max_element()).max(1e-3)),
        ColliderShape::Capsule { height, radius } => {
            let r = (radius * radial).max(1e-3);
            let half = ((height * s.y) * 0.5 - r).max(0.0);
            ColliderBuilder::capsule_y(half, r)
        }
        ColliderShape::Cylinder { height, radius } => {
            ColliderBuilder::cylinder((height * s.y * 0.5).max(1e-3), (radius * radial).max(1e-3))
        }
    };
    let mut builder = builder
        .translation(rv(c.offset * s))
        .sensor(c.trigger)
        .friction(c.friction.max(0.0))
        .restitution(c.restitution.clamp(0.0, 1.0))
        .active_events(ActiveEvents::COLLISION_EVENTS)
        // Report touches between every kind of body (kinematic vs static too).
        .active_collision_types(ActiveCollisionTypes::all());
    if let Some(b) = body.filter(|b| b.kind == BodyKind::Dynamic) {
        builder = builder.mass(b.mass.max(1e-3));
    }
    builder.build()
}

/// A heightfield collider for a terrain (scaled by the entity's scale).
fn build_heightfield(t: &Terrain, scale: Vec3) -> rapier3d::prelude::Collider {
    let r = t.resolution as usize;
    let heights = rapier3d::parry::utils::Array2::from_fn(r, r, |z, x| t.heights[z * r + x]);
    let s = scale.abs().max(Vec3::splat(1e-4));
    ColliderBuilder::heightfield(heights, RVec::new(t.size * s.x, s.y, t.size * s.z))
        .friction(0.8)
        .active_events(ActiveEvents::COLLISION_EVENTS)
        .active_collision_types(ActiveCollisionTypes::all())
        .build()
}

impl CollisionWorld {
    pub fn new() -> Self {
        Self {
            world: PhysicsWorld::new(),
            entries: HashMap::new(),
            by_collider: HashMap::new(),
            dynamics: false,
            gravity: Vec3::new(0.0, -9.81, 0.0),
            accumulator: 0.0,
            fixed_dt: 1.0 / 60.0,
            contacts: HashSet::new(),
            pending: Vec::new(),
            forced: HashSet::new(),
        }
    }

    /// Number of entities in the collision world.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn remove(&mut self, e: Entity) {
        if let Some(entry) = self.entries.remove(&e) {
            self.by_collider.remove(&entry.collider);
            self.world.remove_body_with_colliders(entry.body, true);
        }
        self.contacts.retain(|(a, b)| *a != e && *b != e);
    }

    /// Drop everything (play stopped).
    pub fn clear(&mut self) {
        let all: Vec<Entity> = self.entries.keys().copied().collect();
        for e in all {
            self.remove(e);
        }
        self.pending.clear();
        self.accumulator = 0.0;
    }

    /// Bring the collision world in line with the ECS.
    pub fn sync(&mut self, world: &World) {
        let mut live: HashSet<Entity> = world.query::<(Collider,)>().into_iter().collect();
        if let Some(terrains) = world.storage::<Terrain>() {
            live.extend(terrains.iter().filter(|(_, t)| t.collide).map(|(e, _)| e));
        }
        let stale: Vec<Entity> = self.entries.keys().filter(|e| !live.contains(e)).copied().collect();
        for e in stale {
            self.remove(e);
        }
        for &e in &live {
            let Some(m) = world.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
            let (scale, rot, pos) = decompose(m);
            let terrain = world.get::<Terrain>(e).filter(|t| t.collide).map(|t| t.fingerprint());
            let collider_cfg = world.get_cloned::<Collider>(e).unwrap_or_default();
            // Terrains are static ground.
            let body_cfg = if terrain.is_some() {
                Some(RigidBody { kind: BodyKind::Static, ..Default::default() })
            } else if self.dynamics {
                world.get_cloned::<RigidBody>(e)
            } else {
                None
            };
            let rebuild = match self.entries.get(&e) {
                None => true,
                Some(en) => {
                    en.collider_cfg != collider_cfg
                        || en.body_cfg != body_cfg
                        || en.terrain != terrain
                        || !en.scale.abs_diff_eq(scale, 1e-4)
                }
            };
            if rebuild {
                self.remove(e);
                let kind = body_cfg.as_ref().map(|b| b.kind);
                let builder = match kind {
                    Some(BodyKind::Dynamic) => RigidBodyBuilder::dynamic(),
                    Some(BodyKind::Static) => RigidBodyBuilder::fixed(),
                    // Kinematic bodies and plain colliders follow their transform.
                    Some(BodyKind::Kinematic) | None => RigidBodyBuilder::kinematic_position_based(),
                };
                let mut builder = builder.pose(rpose(pos, rot)).user_data(e.to_bits() as u128);
                if let Some(b) = &body_cfg {
                    let mut locked = LockedAxes::empty();
                    if b.lock_rotation {
                        locked |= LockedAxes::ROTATION_LOCKED;
                    }
                    if b.lock_2d {
                        locked |= LockedAxes::TRANSLATION_LOCKED_Z
                            | LockedAxes::ROTATION_LOCKED_X
                            | LockedAxes::ROTATION_LOCKED_Y;
                    }
                    builder = builder
                        .gravity_scale(b.gravity_scale)
                        .linear_damping(b.linear_damping.max(0.0))
                        .angular_damping(b.angular_damping.max(0.0))
                        .locked_axes(locked)
                        .ccd_enabled(b.ccd);
                }
                let body = self.world.insert_body(builder);
                let shape = match terrain {
                    Some(_) => build_heightfield(&world.get::<Terrain>(e).unwrap(), scale),
                    None => build_collider(&collider_cfg, scale, body_cfg.as_ref()),
                };
                let collider = self.world.insert_collider(shape, Some(body));
                self.by_collider.insert(collider, e);
                self.entries.insert(
                    e,
                    Entry {
                        body,
                        collider,
                        dynamic: kind == Some(BodyKind::Dynamic),
                        collider_cfg,
                        body_cfg,
                        scale,
                        terrain,
                        last: (pos, rot),
                    },
                );
                continue;
            }
            let en = self.entries.get_mut(&e).unwrap();
            let Some(body) = self.world.bodies.get_mut(en.body) else { continue };
            let now = (pos, rot);
            match body.body_type() {
                RigidBodyType::KinematicPositionBased => body.set_next_kinematic_position(rpose(pos, rot)),
                _ => {
                    // A script (or the editor) moved the entity: teleport the body.
                    if !close(now, en.last) {
                        body.set_position(rpose(pos, rot), true);
                    }
                }
            }
            en.last = now;
        }
        // Commands issued before the body existed.
        for (e, cmd) in std::mem::take(&mut self.pending) {
            if self.entries.contains_key(&e) {
                self.apply(e, cmd);
            }
        }
    }

    fn apply(&mut self, e: Entity, cmd: Pending) {
        let Some(en) = self.entries.get(&e) else {
            self.pending.push((e, cmd));
            return;
        };
        let Some(body) = self.world.bodies.get_mut(en.body) else { return };
        match cmd {
            Pending::LinVel(v) => body.set_linvel(rv(v), true),
            Pending::AngVel(v) => body.set_angvel(rv(v), true),
            Pending::Impulse(v) => body.apply_impulse(rv(v), true),
            Pending::Force(v) => {
                body.add_force(rv(v), true);
                self.forced.insert(e);
            }
        }
    }

    /// Advance by `dt` seconds in fixed steps; returns this frame's collision events.
    pub fn step(&mut self, dt: f32) -> Vec<CollisionEvent> {
        self.world.gravity = if self.dynamics { rv(self.gravity) } else { RVec::ZERO };
        self.world.integration_parameters.dt = self.fixed_dt;
        self.accumulator = (self.accumulator + dt).min(self.fixed_dt * 5.0);
        let (tx, rx) = std::sync::mpsc::channel();
        let (ftx, _frx) = std::sync::mpsc::channel();
        let (stx, _srx) = std::sync::mpsc::channel();
        let collector = ChannelEventCollector::new(tx, ftx, stx);
        while self.accumulator >= self.fixed_dt {
            self.accumulator -= self.fixed_dt;
            self.world.step_with_events(&(), &collector);
        }
        for e in self.forced.drain() {
            if let Some(body) = self.entries.get(&e).and_then(|en| self.world.bodies.get_mut(en.body)) {
                body.reset_forces(false);
            }
        }
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            let (h1, h2, flags, started) = match ev {
                rapier3d::prelude::CollisionEvent::Started(a, b, f) => (a, b, f, true),
                rapier3d::prelude::CollisionEvent::Stopped(a, b, f) => (a, b, f, false),
            };
            let (Some(&a), Some(&b)) = (self.by_collider.get(&h1), self.by_collider.get(&h2)) else { continue };
            if started {
                self.contacts.insert(pair(a, b));
            } else {
                self.contacts.remove(&pair(a, b));
            }
            events.push(CollisionEvent { a, b, started, trigger: flags.contains(CollisionEventFlags::SENSOR) });
        }
        events
    }

    /// Write simulated poses of dynamic bodies back to their transforms.
    pub fn write_back(&mut self, world: &mut World) {
        let mut moved = false;
        for (&e, en) in self.entries.iter_mut() {
            if !en.dynamic || !world.is_alive(e) {
                continue;
            }
            let Some(body) = self.world.bodies.get(en.body) else { continue };
            let p = body.position();
            let pos = gv(p.translation);
            let rot = gq(p.rotation);
            if close((pos, rot), en.last) {
                continue;
            }
            kit_scene::set_world_matrix(world, e, Mat4::from_scale_rotation_translation(en.scale, rot, pos));
            en.last = (pos, rot);
            moved = true;
        }
        if moved {
            kit_scene::propagate_transforms(world);
        }
    }

    fn body(&self, e: Entity) -> Option<&rapier3d::prelude::RigidBody> {
        self.entries.get(&e).and_then(|en| self.world.bodies.get(en.body))
    }
}

impl PhysicsApi for CollisionWorld {
    fn linear_velocity(&self, e: Entity) -> Option<Vec3> {
        self.body(e).map(|b| gv(b.linvel()))
    }
    fn set_linear_velocity(&mut self, e: Entity, v: Vec3) {
        self.apply(e, Pending::LinVel(v));
    }
    fn angular_velocity(&self, e: Entity) -> Option<Vec3> {
        self.body(e).map(|b| gv(b.angvel()))
    }
    fn set_angular_velocity(&mut self, e: Entity, v: Vec3) {
        self.apply(e, Pending::AngVel(v));
    }
    fn apply_impulse(&mut self, e: Entity, impulse: Vec3) {
        self.apply(e, Pending::Impulse(impulse));
    }
    fn add_force(&mut self, e: Entity, force: Vec3) {
        self.apply(e, Pending::Force(force));
    }
    fn raycast(&self, origin: Vec3, dir: Vec3, max_distance: f32, exclude: Option<Entity>) -> Option<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        let mut filter = QueryFilter::default().exclude_sensors();
        if let Some(en) = exclude.and_then(|e| self.entries.get(&e)) {
            filter = filter.exclude_rigid_body(en.body);
        }
        let ray = Ray::new(rv(origin), rv(dir));
        let (h, hit) = self.world.cast_ray_and_get_normal(&ray, max_distance, true, filter)?;
        let entity = *self.by_collider.get(&h)?;
        let distance = hit.time_of_impact;
        Some(RayHit { entity, distance, point: origin + dir * distance, normal: gv(hit.normal) })
    }
    fn contacts(&self, e: Entity) -> Vec<Entity> {
        let mut out: Vec<Entity> = self
            .contacts
            .iter()
            .filter_map(|(a, b)| {
                if *a == e {
                    Some(*b)
                } else if *b == e {
                    Some(*a)
                } else {
                    None
                }
            })
            .collect();
        out.sort();
        out
    }
    fn suspend_gravity(&mut self, e: Entity, suspended: bool) {
        let Some(en) = self.entries.get(&e) else { return };
        let scale = if suspended { 0.0 } else { en.body_cfg.as_ref().map(|b| b.gravity_scale).unwrap_or(1.0) };
        if let Some(body) = self.world.bodies.get_mut(en.body) {
            if (body.gravity_scale() - scale).abs() > 1e-6 {
                body.set_gravity_scale(scale, true);
            }
        }
    }
    fn gravity(&self) -> Vec3 {
        self.gravity
    }
    fn set_gravity(&mut self, g: Vec3) {
        self.gravity = g;
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Run `f` with the world's [`CollisionWorld`], if the collider module is loaded.
pub fn with_collision_world<R>(world: &mut World, f: impl FnOnce(&mut CollisionWorld, &mut World) -> R) -> Option<R> {
    let mut physics = world.remove_resource::<Physics>()?;
    let result = physics.0.as_any_mut().downcast_mut::<CollisionWorld>().map(|cw| f(cw, world));
    world.insert_resource(physics);
    result
}

/// The per-frame collision system (play mode).
pub fn collision_step(world: &mut World) {
    let dt = world.resource::<Time>().delta;
    kit_scene::propagate_transforms(world);
    let events = with_collision_world(world, |cw, world| {
        cw.sync(world);
        let events = cw.step(dt);
        cw.write_back(world);
        events
    });
    if let Some(events) = events {
        world.resource_or_default::<CollisionEvents>().events = events;
    }
}

/// Remove all bodies and pending events (call when play stops).
pub fn reset(world: &mut World) {
    with_collision_world(world, |cw, _| cw.clear());
    if let Some(mut ev) = world.try_resource_mut::<CollisionEvents>() {
        ev.events.clear();
    }
}

/// Collision detection. Add after the script module so it runs after scripts.
pub struct ColliderModule;

impl Module for ColliderModule {
    fn name(&self) -> &'static str {
        "collider"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["scene", "gamesys"]
    }
    fn build(&self, app: &mut App) {
        app.world.insert_resource(Physics(Box::new(CollisionWorld::new())));
        app.world.insert_resource(CollisionEvents::default());
        app.add_play_system(Stage::Update, "collision_step", collision_step);
    }
}

#[cfg(test)]
mod tests;
