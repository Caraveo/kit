use super::*;
use kit_gamesys::GameSysModule;
use kit_scene::{SceneModule, Transform};

fn app() -> App {
    let mut app = App::new();
    app.add_module(SceneModule).add_module(GameSysModule).add_module(ColliderModule);
    app
}

#[test]
fn colliders_without_physics_report_triggers_and_do_not_fall() {
    let mut app = app();
    let w = &mut app.world;
    let zone = kit_scene::spawn_node(w, "Zone", Transform::from_xyz(5.0, 0.0, 0.0));
    w.insert(
        zone,
        Collider { trigger: true, shape: ColliderShape::Box { size: Vec3::splat(2.0) }, ..Default::default() },
    );
    let mover = kit_scene::spawn_node(w, "Mover", Transform::from_xyz(0.0, 0.0, 0.0));
    w.insert(mover, Collider { shape: ColliderShape::Sphere { radius: 0.5 }, ..Default::default() });
    // A RigidBody is ignored without the physics module: no gravity.
    w.insert(mover, RigidBody::default());

    let mut started = Vec::new();
    for i in 0..60 {
        app.world.get_mut::<Transform>(mover).unwrap().translation.x = i as f32 * 0.1;
        app.update(1.0 / 60.0);
        started.extend(app.world.resource::<CollisionEvents>().events.iter().filter(|e| e.started).copied());
    }
    assert_eq!(started.len(), 1, "{started:?}");
    let ev = started[0];
    assert!(ev.trigger);
    assert_eq!(pair(ev.a, ev.b), pair(zone, mover));
    assert_eq!(app.world.get::<Transform>(mover).unwrap().translation.y, 0.0);
}

#[test]
fn raycast_hits_nearest_and_respects_exclude() {
    let mut app = app();
    let w = &mut app.world;
    let ground =
        kit_scene::spawn_node(w, "Ground", Transform::from_xyz(0.0, -0.5, 0.0).with_scale(Vec3::new(10.0, 1.0, 10.0)));
    w.insert(ground, Collider::default());
    let crate_ = kit_scene::spawn_node(w, "Crate", Transform::from_xyz(0.0, 1.0, 0.0));
    w.insert(crate_, Collider::default());
    app.update(1.0 / 60.0);
    let physics = app.world.resource::<Physics>();
    let hit = physics.0.raycast(Vec3::new(0.0, 10.0, 0.0), Vec3::NEG_Y, 100.0, None).unwrap();
    assert_eq!(hit.entity, crate_);
    assert!((hit.distance - 8.5).abs() < 1e-3, "{hit:?}");
    assert!((hit.normal - Vec3::Y).length() < 1e-3);
    let hit = physics.0.raycast(Vec3::new(0.0, 10.0, 0.0), Vec3::NEG_Y, 100.0, Some(crate_)).unwrap();
    assert_eq!(hit.entity, ground);
    assert!(physics.0.raycast(Vec3::new(0.0, 10.0, 0.0), Vec3::Y, 100.0, None).is_none());
}

#[test]
fn removed_entities_leave_the_world() {
    let mut app = app();
    let e = kit_scene::spawn_node(&mut app.world, "A", Transform::IDENTITY);
    app.world.insert(e, Collider::default());
    app.update(1.0 / 60.0);
    let count = |app: &mut App| with_collision_world(&mut app.world, |cw, _| cw.len()).unwrap();
    assert_eq!(count(&mut app), 1);
    app.world.despawn(e);
    app.update(1.0 / 60.0);
    assert_eq!(count(&mut app), 0);
}
