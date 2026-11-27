use super::*;
use kit_core::RunMode;
use kit_gamesys::GameSysModule;
use kit_scene::{SceneModule, Transform};

fn app() -> App {
    let mut app = App::new();
    app.add_module(SceneModule).add_module(GameSysModule).add_module(ParticlesModule);
    app
}

fn emitter(app: &mut App, e: ParticleEmitter) -> Entity {
    let ent = kit_scene::spawn_node(&mut app.world, "Sparks", Transform::from_xyz(1.0, 2.0, 3.0));
    app.world.insert(ent, e);
    ent
}

fn count(app: &App, e: Entity) -> usize {
    app.world.get::<ParticleState>(e).map_or(0, |s| s.particles.len())
}

fn step(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update(1.0 / 60.0);
    }
}

#[test]
fn bursts_on_start_then_emits_at_its_rate_and_fades() {
    let mut app = app();
    app.mode = RunMode::Play;
    let e = emitter(&mut app, ParticleEmitter { rate: 60.0, burst: 25, lifetime: 2.0, ..Default::default() });
    step(&mut app, 1);
    assert_eq!(count(&app, e), 26, "the burst plus one frame of emission");
    step(&mut app, 59);
    let n = count(&app, e);
    assert!((83..=86).contains(&n), "25 + ~60 a second: {n}");
    // Particles start at the emitter and fly up (the default direction), within the cone.
    let st = app.world.get::<ParticleState>(e).unwrap();
    assert!(st.particles.iter().all(|p| p.position.y > 2.0 - 1.0));
    let newest = st.particles.last().unwrap();
    let oldest = st.particles.first().unwrap();
    assert!(
        newest.opacity() > 0.95 && oldest.opacity() < 0.55,
        "fade over life: {} {}",
        newest.opacity(),
        oldest.opacity()
    );
    drop(st);
    // After the lifetime the burst is gone; only the steady stream is left.
    step(&mut app, 120);
    let n = count(&app, e);
    assert!((118..=122).contains(&n), "steady: rate x lifetime = 120: {n}");
}

#[test]
fn launch_direction_turns_with_the_object_and_spread_zero_is_a_line() {
    let mut app = app();
    app.mode = RunMode::Play;
    let e = emitter(
        &mut app,
        ParticleEmitter {
            rate: 0.0,
            burst: 20,
            spread_degrees: 0.0,
            gravity: Vec3::ZERO,
            direction: Vec3::Y,
            speed: 5.0,
            ..Default::default()
        },
    );
    // Lie the object on its side: up becomes -X.
    app.world.get_mut::<Transform>(e).unwrap().rotation = glam::Quat::from_rotation_z(90f32.to_radians());
    step(&mut app, 30);
    let st = app.world.get::<ParticleState>(e).unwrap();
    assert_eq!(st.particles.len(), 20);
    for p in &st.particles {
        assert!(p.velocity.normalize().dot(Vec3::NEG_X) > 0.999, "{:?}", p.velocity);
        assert!((p.position.y - 2.0).abs() < 1e-3, "a straight line, no gravity");
    }
}

#[test]
fn gravity_pulls_and_paused_game_time_freezes() {
    let mut app = app();
    app.mode = RunMode::Play;
    let e = emitter(
        &mut app,
        ParticleEmitter {
            rate: 0.0,
            burst: 1,
            speed: 0.0,
            gravity: Vec3::new(0.0, -10.0, 0.0),
            lifetime: 10.0,
            ..Default::default()
        },
    );
    step(&mut app, 60);
    let y = app.world.get::<ParticleState>(e).unwrap().particles[0].position.y;
    assert!((y - (2.0 - 5.0)).abs() < 0.3, "fell about 5 units in a second: {y}");
    app.world.resource_mut::<Time>().scale = 0.0;
    step(&mut app, 60);
    let paused = app.world.get::<ParticleState>(e).unwrap().particles[0];
    assert_eq!(paused.position.y, y, "nothing moves while paused");
    app.world.resource_mut::<Time>().scale = 1.0;
    step(&mut app, 1);
    assert!(app.world.get::<ParticleState>(e).unwrap().particles[0].position.y < y);
}

#[test]
fn counts_stay_bounded() {
    let mut app = app();
    app.mode = RunMode::Play;
    let e =
        emitter(&mut app, ParticleEmitter { rate: 100_000.0, burst: 1_000_000, lifetime: 60.0, ..Default::default() });
    step(&mut app, 30);
    assert_eq!(count(&app, e), MAX_PER_EMITTER);
    // Many emitters share one world budget.
    for _ in 0..40 {
        emitter(&mut app, ParticleEmitter { rate: 100_000.0, burst: 10_000, lifetime: 60.0, ..Default::default() });
    }
    step(&mut app, 10);
    assert!(total(&app.world) <= MAX_TOTAL, "{}", total(&app.world));
    assert_eq!(total(&app.world), MAX_TOTAL);
}

#[test]
fn preview_while_editing_and_fresh_state_every_play_session() {
    let mut app = app();
    // A new App plays (that's how exported players start); the editor edits.
    app.mode = RunMode::Edit;
    let e = emitter(&mut app, ParticleEmitter { rate: 30.0, burst: 10, lifetime: 5.0, ..Default::default() });
    // Editing: previews on real time (game time is zero while not playing).
    step(&mut app, 60);
    let preview = count(&app, e);
    assert!(preview >= 38, "preview: {preview}");
    // Play: starts from nothing, with the burst again.
    app.mode = RunMode::Play;
    step(&mut app, 1);
    assert_eq!(count(&app, e), 10, "fresh: just the burst (plus nothing yet from the 30/s rate)");
    step(&mut app, 30);
    // Stop: the play particles go; preview starts over.
    app.mode = RunMode::Edit;
    step(&mut app, 1);
    assert!(count(&app, e) <= 11, "fresh preview after stop: {}", count(&app, e));
}

#[test]
fn switching_off_clears_and_switching_on_bursts_again() {
    let mut app = app();
    app.mode = RunMode::Play;
    let e = emitter(&mut app, ParticleEmitter { rate: 0.0, burst: 7, lifetime: 30.0, ..Default::default() });
    step(&mut app, 5);
    assert_eq!(count(&app, e), 7);
    app.world.get_mut::<ParticleEmitter>(e).unwrap().enabled = false;
    step(&mut app, 1);
    assert_eq!(count(&app, e), 0);
    app.world.get_mut::<ParticleEmitter>(e).unwrap().enabled = true;
    step(&mut app, 1);
    assert_eq!(count(&app, e), 7, "a burst on activation");
    // An inactive object (BeActive(FALSE)) too.
    app.world.insert(e, kit_scene::Inactive);
    step(&mut app, 1);
    assert_eq!(count(&app, e), 0);
    app.world.remove::<kit_scene::Inactive>(e);
    step(&mut app, 1);
    assert_eq!(count(&app, e), 7);
    // Removing the emitter removes its particles.
    app.world.remove::<ParticleEmitter>(e);
    step(&mut app, 1);
    assert!(!app.world.has::<ParticleState>(e));
}

#[test]
fn settings_save_with_scenes_and_fabs_but_live_particles_never_do() {
    let mut app = app();
    app.mode = RunMode::Play;
    let custom = ParticleEmitter {
        rate: 12.5,
        burst: 40,
        lifetime: 0.75,
        speed: 6.0,
        direction: Vec3::new(1.0, 0.0, 0.0),
        spread_degrees: 90.0,
        gravity: Vec3::new(0.0, -9.81, 0.0),
        size: 0.4,
        color: glam::Vec4::new(0.2, 0.9, 1.0, 0.8),
        texture: "sprites/spark.png".into(),
        enabled: true,
    };
    let e = emitter(&mut app, custom.clone());
    step(&mut app, 10);
    assert!(count(&app, e) > 0);
    let json = kit_gamesys::save_scene(&app.world).to_json();
    assert!(json.contains("\"particle_emitter\"") && json.contains("sprites/spark.png"), "{json}");
    assert!(!json.contains("velocity") && !json.contains("\"age\""), "no live state in the file");
    let fab = kit_gamesys::save_subtree(&app.world, e).to_json();
    assert!(fab.contains("\"particle_emitter\""));

    let mut other = self::app();
    let loaded = kit_gamesys::load_scene(&mut other.world, &kit_gamesys::SceneFile::from_json(&json).unwrap()).unwrap();
    let back = loaded.iter().find_map(|&l| other.world.get_cloned::<ParticleEmitter>(l)).unwrap();
    assert_eq!(back, custom);
    assert!(loaded.iter().all(|&l| !other.world.has::<ParticleState>(l)));
    // Old files without the new fields still load with defaults.
    let partial: ParticleEmitter = serde_json_free_parse();
    assert_eq!(partial.rate, ParticleEmitter::default().rate);
}

/// A scene-file entity with only part of an emitter's settings.
fn serde_json_free_parse() -> ParticleEmitter {
    let scene = kit_gamesys::SceneFile::from_json(
        r#"{"version":1,"entities":[{"id":0,"name":"E","particle_emitter":{"burst":3}}]}"#,
    )
    .unwrap();
    let mut world = self::app();
    let spawned = kit_gamesys::load_scene(&mut world.world, &scene).unwrap();
    let e = world.world.get_cloned::<ParticleEmitter>(spawned[0]).unwrap();
    assert_eq!(e.burst, 3);
    e
}
