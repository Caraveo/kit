//! # kit_physics
//!
//! Rigid-body dynamics on top of the `kit_collider` module. With physics
//! loaded, entities with a [`RigidBody`](kit_gamesys::RigidBody) and a
//! [`Collider`](kit_gamesys::Collider) are simulated:
//!
//! * **Dynamic** bodies fall under gravity, collide, bounce and can be pushed
//!   with forces and impulses (`Physics.ApplyImpulse` in x).
//! * **Kinematic** bodies are moved by scripts and push dynamic ones.
//! * **Static** bodies never move.
//!
//! Colliders without a rigid body keep working as in `kit_collider`: they
//! follow their transforms and push dynamic bodies like kinematic ones.

use glam::Vec3;
use kit_collider::with_collision_world;
use kit_core::{App, Module};

pub struct PhysicsModule {
    /// Gravity in m/s² (from `kit.project` `[physics] gravity`).
    pub gravity: Vec3,
    /// Simulation steps per second.
    pub rate: u32,
    /// Simulate rigid bodies (colliders keep detecting contacts either way).
    pub enabled: bool,
}

impl Default for PhysicsModule {
    fn default() -> Self {
        Self { gravity: Vec3::new(0.0, -9.81, 0.0), rate: 60, enabled: true }
    }
}

/// World-wide physics settings, changeable at any time (also while playing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsSettings {
    pub enabled: bool,
    pub gravity: Vec3,
    pub rate: u32,
}

/// Apply settings to the running collision world.
pub fn apply_settings(world: &mut kit_core::kit_ecs::World, s: PhysicsSettings) {
    with_collision_world(world, |cw, _| {
        cw.dynamics = s.enabled;
        cw.fixed_dt = 1.0 / s.rate.clamp(10, 1000) as f32;
        kit_gamesys::PhysicsApi::set_gravity(cw, s.gravity);
    });
}

/// Current settings of the running collision world.
pub fn settings(world: &mut kit_core::kit_ecs::World) -> Option<PhysicsSettings> {
    with_collision_world(world, |cw, _| PhysicsSettings {
        enabled: cw.dynamics,
        gravity: kit_gamesys::PhysicsApi::gravity(cw),
        rate: (1.0 / cw.fixed_dt).round() as u32,
    })
}

impl Module for PhysicsModule {
    fn name(&self) -> &'static str {
        "physics"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["collider"]
    }
    fn build(&self, app: &mut App) {
        apply_settings(
            &mut app.world,
            PhysicsSettings { enabled: self.enabled, gravity: self.gravity, rate: self.rate },
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn settings_roundtrip_and_disable() {
        let mut app = App::new();
        app.add_module(kit_scene::SceneModule).add_module(GameSysModule).add_module(kit_collider::ColliderModule);
        app.add_module(PhysicsModule::default());
        let s = PhysicsSettings { enabled: true, gravity: Vec3::new(0.0, -3.0, 0.0), rate: 120 };
        apply_settings(&mut app.world, s);
        assert_eq!(settings(&mut app.world), Some(s));
        // Disabled: bodies stay put.
        apply_settings(&mut app.world, PhysicsSettings { enabled: false, ..s });
        let b = kit_scene::spawn_node(&mut app.world, "B", Transform::from_xyz(0.0, 5.0, 0.0));
        app.world.insert(b, Collider::default());
        app.world.insert(b, RigidBody::default());
        for _ in 0..30 {
            app.update(1.0 / 60.0);
        }
        assert_eq!(app.world.get::<Transform>(b).unwrap().translation.y, 5.0);
    }

    use super::*;
    use kit_gamesys::{BodyKind, Collider, ColliderShape, GameSysModule, Physics, RigidBody};
    use kit_scene::{SceneModule, Transform};

    #[test]
    fn ball_falls_and_rests_on_ground() {
        let mut app = App::new();
        app.add_module(SceneModule).add_module(GameSysModule).add_module(kit_collider::ColliderModule);
        app.add_module(PhysicsModule::default());
        let w = &mut app.world;
        let ground = kit_scene::spawn_node(
            w,
            "Ground",
            Transform::from_xyz(0.0, -0.5, 0.0).with_scale(Vec3::new(20.0, 1.0, 20.0)),
        );
        w.insert(ground, Collider::default());
        w.insert(ground, RigidBody { kind: BodyKind::Static, ..Default::default() });
        let ball = kit_scene::spawn_node(w, "Ball", Transform::from_xyz(0.0, 5.0, 0.0));
        w.insert(
            ball,
            Collider { shape: ColliderShape::Sphere { radius: 0.5 }, restitution: 0.0, ..Default::default() },
        );
        w.insert(ball, RigidBody::default());
        for _ in 0..240 {
            app.update(1.0 / 60.0);
        }
        let y = app.world.get::<Transform>(ball).unwrap().translation.y;
        assert!((y - 0.5).abs() < 0.05, "ball should rest on the ground, y = {y}");
        let touching = app.world.resource::<Physics>().0.contacts(ball);
        assert_eq!(touching, vec![ground]);
    }

    #[test]
    fn collisions_report_where_and_how_hard_and_queries_find_things() {
        let mut app = App::new();
        app.add_module(SceneModule).add_module(GameSysModule).add_module(kit_collider::ColliderModule);
        app.add_module(PhysicsModule::default());
        let w = &mut app.world;
        let ground = kit_scene::spawn_node(
            w,
            "Ground",
            Transform::from_xyz(0.0, -0.5, 0.0).with_scale(Vec3::new(20.0, 1.0, 20.0)),
        );
        w.insert(ground, Collider { layer: 3, ..Default::default() });
        w.insert(ground, RigidBody { kind: BodyKind::Static, ..Default::default() });
        let ball = kit_scene::spawn_node(w, "Ball", Transform::from_xyz(2.0, 3.0, 0.0));
        w.insert(
            ball,
            Collider { shape: ColliderShape::Sphere { radius: 0.5 }, restitution: 0.0, ..Default::default() },
        );
        w.insert(ball, RigidBody::default());
        let mut hit = None;
        for _ in 0..120 {
            app.update(1.0 / 60.0);
            if let Some(ev) = app.world.resource::<kit_gamesys::CollisionEvents>().events.iter().find(|e| e.started) {
                hit = Some(*ev);
            }
        }
        let ev = hit.expect("the ball lands");
        // Normal points from a to b; the contact is under the ball, on the ground's top.
        let (ball_side, normal) = if ev.a == ball { (ev.a, ev.normal) } else { (ev.b, -ev.normal) };
        assert_eq!(ball_side, ball);
        assert!(normal.y < -0.9, "from the ball down into the ground: {normal:?}");
        assert!((ev.point.x - 2.0).abs() < 0.3 && ev.point.y.abs() < 0.2, "{:?}", ev.point);
        assert!(ev.strength > 0.0, "it hit with some force");

        let phys = app.world.resource::<Physics>();
        // FindNear: everything touching a sphere; layers filter.
        let near = phys.0.overlap_sphere(Vec3::new(2.0, 0.5, 0.0), 1.0, None, u32::MAX);
        assert!(near.contains(&ball) && near.contains(&ground), "{near:?}");
        assert_eq!(phys.0.overlap_sphere(Vec3::new(2.0, 0.5, 0.0), 1.0, None, 1 << 3), vec![ground]);
        // SweepSphere: a fat ray.
        let sweep = phys.0.sphere_cast(Vec3::new(-5.0, 0.5, 0.0), Vec3::X, 0.25, 50.0, None, u32::MAX).unwrap();
        assert_eq!(sweep.entity, ball);
        assert!((sweep.distance - 6.25).abs() < 0.1, "{}", sweep.distance);
        // Raycast only on layer 3 skips the ball and finds the ground.
        let down = phys.0.raycast_layers(Vec3::new(2.0, 5.0, 0.0), Vec3::NEG_Y, 20.0, None, 1 << 3).unwrap();
        assert_eq!(down.entity, ground);
        drop(phys);

        // Switching a body to kinematic: it stops falling and follows its transform.
        app.world.get_mut::<RigidBody>(ball).unwrap().kind = BodyKind::Kinematic;
        app.world.get_mut::<Transform>(ball).unwrap().translation = Vec3::new(2.0, 4.0, 0.0);
        for _ in 0..30 {
            app.update(1.0 / 60.0);
        }
        assert!((app.world.get::<Transform>(ball).unwrap().translation.y - 4.0).abs() < 1e-3);

        // A switched-off collider leaves the world; switched back on, it returns.
        app.world.insert(ball, kit_gamesys::DisabledFeatures(vec!["Collider".into()]));
        app.update(1.0 / 60.0);
        assert!(
            !app.world
                .resource::<Physics>()
                .0
                .overlap_sphere(Vec3::new(2.0, 4.0, 0.0), 0.1, None, u32::MAX)
                .contains(&ball)
        );
        app.world.remove::<kit_gamesys::DisabledFeatures>(ball);
        app.update(1.0 / 60.0);
        assert!(
            app.world
                .resource::<Physics>()
                .0
                .overlap_sphere(Vec3::new(2.0, 4.0, 0.0), 0.1, None, u32::MAX)
                .contains(&ball)
        );
    }

    #[test]
    fn ball_rolls_down_a_terrain_hill() {
        use kit_gamesys::Terrain;
        let mut app = App::new();
        app.add_module(SceneModule).add_module(GameSysModule).add_module(kit_collider::ColliderModule);
        app.add_module(PhysicsModule::default());
        let w = &mut app.world;
        // A ramp: height rises along +x.
        let mut t = Terrain::flat(20.0, 21);
        for z in 0..21 {
            for x in 0..21 {
                t.heights[z * 21 + x] = x as f32 * 0.5;
            }
        }
        let ground = kit_scene::spawn_node(w, "Terrain", Transform::IDENTITY);
        w.insert(ground, t.clone());
        let ball = kit_scene::spawn_node(w, "Ball", Transform::from_xyz(5.0, 9.0, 0.0));
        w.insert(ball, Collider { shape: ColliderShape::Sphere { radius: 0.5 }, ..Default::default() });
        w.insert(ball, RigidBody::default());
        for _ in 0..180 {
            app.update(1.0 / 60.0);
        }
        let p = app.world.get::<Transform>(ball).unwrap().translation;
        let ground_h = t.height_at(glam::Vec2::new(p.x, p.z)).unwrap();
        assert!(p.x < 4.0, "rolled downhill (toward -x): {p}");
        assert!(p.y > ground_h - 0.05 && p.y < ground_h + 1.0, "resting on the surface: {p} ground {ground_h}");
    }

    #[test]
    fn impulse_and_2d_lock() {
        let mut app = App::new();
        app.add_module(SceneModule).add_module(GameSysModule).add_module(kit_collider::ColliderModule);
        app.add_module(PhysicsModule { gravity: Vec3::ZERO, ..Default::default() });
        let w = &mut app.world;
        let b = kit_scene::spawn_node(w, "Box", Transform::IDENTITY);
        w.insert(b, Collider::default());
        w.insert(b, RigidBody { lock_2d: true, ..Default::default() });
        // Issued before the body exists: applied on the first step.
        app.world.resource_mut::<Physics>().0.apply_impulse(b, Vec3::new(2.0, 0.0, 5.0));
        for _ in 0..60 {
            app.update(1.0 / 60.0);
        }
        let p = app.world.get::<Transform>(b).unwrap().translation;
        assert!((p.x - 2.0).abs() < 0.1, "moved along x: {p}");
        assert!(p.z.abs() < 1e-3, "z is locked in 2D: {p}");
    }
}
