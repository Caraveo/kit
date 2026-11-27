use super::*;
use kit_gamesys::GameSysModule;
use kit_input::{Input, InputModule};
use kit_resource::ResourceModule;
use kit_scene::SceneModule;

const LOCK: &str = "version = 1\n\n[[circuit]]\nid = \"kit.cinematics\"\nversion = \"0.1.0\"\nsha256 = \"x\"\nsource = \"test\"\nenabled = true\n";

fn project(lock: Option<&str>) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kit_cine_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(l) = lock {
        std::fs::write(dir.join("circuits.lock"), l).unwrap();
    }
    dir
}

fn app(lock: Option<&str>) -> App {
    let mut app = App::new();
    app.add_module(ResourceModule { root: project(lock) })
        .add_module(SceneModule)
        .add_module(InputModule)
        .add_module(GameSysModule)
        .add_module(kit_path::PathModule)
        .add_module(CinematicsModule);
    app.mode = kit_core::RunMode::Play;
    app
}

fn extra(key: &str, v: serde_json::Value) -> ExtraData {
    let mut m = serde_json::Map::new();
    m.insert(key.into(), v);
    ExtraData(m)
}

struct Scene {
    game_cam: Entity,
    shot1: Entity,
    shot2: Entity,
    target: Entity,
}

fn scene(app: &mut App, play_on_start: bool) -> Scene {
    let w = &mut app.world;
    let game_cam = kit_scene::spawn_node(w, "Camera", Transform::from_xyz(0.0, 2.0, 10.0));
    w.insert(game_cam, Camera::default());
    let target = kit_scene::spawn_node(w, "Hero", Transform::from_xyz(5.0, 0.0, 0.0));
    let director = kit_scene::spawn_node(w, "Intro", Transform::IDENTITY);
    w.insert(
        director,
        extra(DIRECTOR, serde_json::json!({ "play_on_start": play_on_start, "fade_in": 0.5, "fade_out": 0.5 })),
    );
    let shot1 = kit_scene::spawn_node(w, "Wide", Transform::from_xyz(-20.0, 5.0, 0.0));
    w.insert(shot1, Camera { active: false, ..Camera::default() });
    w.insert(shot1, extra(SHOT, serde_json::json!({ "duration": 1.0, "look_at": "Hero", "caption": "Long ago", "zoom_to": 30.0 })));
    kit_scene::set_parent(w, shot1, director);
    let shot2 = kit_scene::spawn_node(w, "Close", Transform::from_xyz(5.0, 1.0, 4.0));
    w.insert(shot2, Camera { active: false, ..Camera::default() });
    w.insert(shot2, extra(SHOT, serde_json::json!({ "duration": 1.0, "transition": "blend", "blend": 0.5 })));
    // A sketched move: it should only run during its shot.
    w.insert(
        shot2,
        PathFollow { points: vec![Vec3::new(5.0, 1.0, 4.0), Vec3::new(5.0, 1.0, -4.0)], speed: 4.0, playing: true, face_direction: false, ..Default::default() },
    );
    kit_scene::set_parent(w, shot2, director);
    Scene { game_cam, shot1, shot2, target }
}

fn step(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update(1.0 / 60.0);
    }
}

fn events(app: &App) -> Vec<(String, Option<BridgeValue>)> {
    app.world.resource::<GraphEvents>().0.iter().map(|e| (e.name.clone(), e.payload.clone())).collect()
}

#[test]
fn plays_shots_in_order_with_overlay_and_gives_the_camera_back() {
    let mut app = app(Some(LOCK));
    let s = scene(&mut app, true);
    step(&mut app, 1);
    assert!(app.world.resource::<CinematicControl>().available);
    assert_eq!(kit_gamesys::active_camera(&app.world), Some(s.shot1), "first shot shows the game");
    assert!(events(&app).iter().any(|(n, p)| n == "CinematicStarted" && *p == Some(BridgeValue::Text("Intro".into()))));
    let o = app.world.resource::<CinematicOverlay>().clone();
    assert_eq!(o.caption, "Long ago");
    assert!(o.fade > 0.8, "fading in from black: {}", o.fade);
    // The path waits for its shot.
    let p0 = app.world.get::<Transform>(s.shot2).unwrap().translation;

    step(&mut app, 30);
    let o = app.world.resource::<CinematicOverlay>().clone();
    assert!(o.letterbox > 0.99 && o.fade < 0.01, "{o:?}");
    // Look at: the wide shot faces the hero; zoom heads toward 30°.
    let t = *app.world.get::<Transform>(s.shot1).unwrap();
    let to_hero = (Vec3::new(5.0, 0.0, 0.0) - t.translation).normalize();
    assert!(t.forward().dot(to_hero) > 0.999, "{:?}", t.forward());
    assert!(fov_of(&app.world, s.shot1).unwrap() < 50.0);
    assert_eq!(app.world.get::<Transform>(s.shot2).unwrap().translation, p0, "path waited");
    let _ = s.target;

    // Second shot: blends in from the wide shot, then follows its path.
    step(&mut app, 32);
    assert_eq!(kit_gamesys::active_camera(&app.world), Some(s.shot2));
    assert_eq!(app.world.resource::<CinematicControl>().shot, 1);
    let mid = app.world.get::<Transform>(s.shot2).unwrap().translation;
    assert!(mid.x < 4.0, "still gliding over from the wide shot: {mid:?}");
    // Past the half-second blend: on its own path (moving from z = 4 toward -4).
    step(&mut app, 34);
    let later = app.world.get::<Transform>(s.shot2).unwrap().translation;
    assert!(later.z < 3.0 && (later.x - 5.0).abs() < 0.2, "on its path: {later:?}");

    // End: black, then the game's camera returns and the screen clears.
    step(&mut app, 26);
    assert!(app.world.resource::<CinematicControl>().playing.is_none());
    assert_eq!(kit_gamesys::active_camera(&app.world), Some(s.game_cam));
    let saw_finished = events(&app).iter().any(|(n, _)| n == "CinematicFinished");
    step(&mut app, 40);
    assert!(saw_finished || events(&app).is_empty());
    assert!(!app.world.resource::<CinematicOverlay>().is_visible());
    // Shot cameras are back where they were authored, zoom undone.
    assert_eq!(app.world.get::<Transform>(s.shot2).unwrap().translation, Vec3::new(5.0, 1.0, 4.0));
    assert_eq!(fov_of(&app.world, s.shot1), Some(50.0));
}

#[test]
fn scripts_and_diagrams_start_skip_and_stop() {
    let mut app = app(Some(LOCK));
    let s = scene(&mut app, false);
    step(&mut app, 2);
    assert_eq!(kit_gamesys::active_camera(&app.world), Some(s.game_cam), "not on start");

    // A diagram's custom event.
    app.world.resource_mut::<GraphEvents>().0.push(GraphEvent {
        name: "PlayCinematic".into(),
        payload: Some(BridgeValue::Text("intro".into())),
        sender: None,
    });
    step(&mut app, 1);
    assert_eq!(app.world.resource::<CinematicControl>().playing.as_deref(), Some("Intro"));
    assert_eq!(kit_gamesys::active_camera(&app.world), Some(s.shot1));

    // The skip key.
    step(&mut app, 10);
    app.world.resource_mut::<Input>().press("Escape");
    step(&mut app, 1);
    app.world.resource_mut::<Input>().release("Escape");
    assert!(app.world.resource::<CinematicControl>().playing.is_none());
    assert_eq!(kit_gamesys::active_camera(&app.world), Some(s.game_cam));

    // A script's request, then Stop.
    app.world.resource_mut::<CinematicControl>().requests.push(CinematicRequest::Play("Intro".into()));
    step(&mut app, 3);
    assert_eq!(kit_gamesys::active_camera(&app.world), Some(s.shot1));
    app.world.resource_mut::<CinematicControl>().requests.push(CinematicRequest::Stop);
    step(&mut app, 1);
    assert_eq!(kit_gamesys::active_camera(&app.world), Some(s.game_cam));
    assert!(!app.world.resource::<CinematicOverlay>().is_visible(), "Stop cuts out at once");

    // Unknown names are a warning, not a crash.
    app.world.resource_mut::<CinematicControl>().requests.push(CinematicRequest::Play("Nope".into()));
    step(&mut app, 1);
    let warned = app.world.resource::<Console>().lines().iter().any(|l| l.text.contains("no cinematic named \"Nope\""));
    assert!(warned);
}

#[test]
fn does_nothing_without_the_circuit() {
    let mut app = app(None);
    let s = scene(&mut app, true);
    step(&mut app, 10);
    assert!(!app.world.resource::<CinematicControl>().available);
    assert_eq!(kit_gamesys::active_camera(&app.world), Some(s.game_cam));
    assert!(!app.world.resource::<CinematicOverlay>().is_visible());
    // Its paths aren't touched either.
    assert!(app.world.get::<PathFollow>(s.shot2).unwrap().playing);
}

#[test]
fn reads_the_lockfile() {
    assert!(enabled_in_lock(LOCK));
    assert!(!enabled_in_lock(&LOCK.replace("enabled = true", "enabled = false")));
    assert!(!enabled_in_lock("version = 1\n"));
}

#[test]
fn blends_continue_from_what_was_on_screen() {
    // Two look-at shots (their cameras' own rotations don't face the target);
    // the second blends in. The view must not jump when the blend starts.
    let mut app = app(Some(LOCK));
    let w = &mut app.world;
    let game_cam = kit_scene::spawn_node(w, "Camera", Transform::from_xyz(0.0, 2.0, 10.0));
    w.insert(game_cam, Camera::default());
    kit_scene::spawn_node(w, "Hero", Transform::from_xyz(0.0, 0.0, 0.0));
    let director = kit_scene::spawn_node(w, "Intro", Transform::IDENTITY);
    w.insert(director, extra(DIRECTOR, serde_json::json!({ "play_on_start": true, "fade_in": 0.0, "fade_out": 0.0 })));
    let a = kit_scene::spawn_node(w, "A", Transform::from_xyz(-10.0, 3.0, 0.0));
    w.insert(a, Camera { active: false, ..Camera::default() });
    w.insert(a, extra(SHOT, serde_json::json!({ "duration": 0.5, "look_at": "Hero" })));
    kit_scene::set_parent(w, a, director);
    let b = kit_scene::spawn_node(w, "B", Transform::from_xyz(10.0, 3.0, 0.0));
    w.insert(b, Camera { active: false, ..Camera::default() });
    w.insert(b, extra(SHOT, serde_json::json!({ "duration": 2.0, "look_at": "Hero", "transition": "blend", "blend": 1.0 })));
    kit_scene::set_parent(w, b, director);

    let view = |app: &App| {
        let cam = kit_gamesys::active_camera(&app.world).unwrap();
        let g = app.world.get::<GlobalTransform>(cam).unwrap().0;
        (cam, g.w_axis.truncate(), g.transform_vector3(Vec3::NEG_Z).normalize())
    };
    let mut last_a = None;
    let mut first_b = None;
    let mut mid_b = None;
    for i in 0..90 {
        app.update(1.0 / 60.0);
        let (cam, pos, fwd) = view(&app);
        if cam == a {
            last_a = Some((pos, fwd));
        } else if cam == b && first_b.is_none() {
            first_b = Some((pos, fwd));
        } else if cam == b && i == 60 {
            mid_b = Some(pos);
        }
    }
    let (pa, fa) = last_a.unwrap();
    let (pb, fb) = first_b.unwrap();
    assert!(pa.distance(pb) < 0.5, "no jump in position: {pa:?} -> {pb:?}");
    assert!(fa.dot(fb) > 0.99, "no jump in direction: {fa:?} -> {fb:?}");
    // Halfway through the blend it's between the two cameras, still looking at the hero.
    let m = mid_b.unwrap();
    assert!(m.x > -9.0 && m.x < 9.0, "gliding across: {m:?}");
}

#[test]
fn zoom_blends_too() {
    // Shot A zooms in to 20°; shot B (60°) blends in: the field of view glides, never pops.
    let mut app = app(Some(LOCK));
    let w = &mut app.world;
    let game_cam = kit_scene::spawn_node(w, "Camera", Transform::from_xyz(0.0, 2.0, 10.0));
    w.insert(game_cam, Camera::default());
    let director = kit_scene::spawn_node(w, "Intro", Transform::IDENTITY);
    w.insert(director, extra(DIRECTOR, serde_json::json!({ "play_on_start": true, "fade_in": 0.0, "fade_out": 0.0 })));
    let a = kit_scene::spawn_node(w, "A", Transform::from_xyz(-10.0, 3.0, 0.0));
    w.insert(a, Camera { active: false, ..Camera::default() });
    w.insert(a, extra(SHOT, serde_json::json!({ "duration": 0.5, "zoom_to": 20.0 })));
    kit_scene::set_parent(w, a, director);
    let b = kit_scene::spawn_node(w, "B", Transform::from_xyz(10.0, 3.0, 0.0));
    w.insert(b, Camera { active: false, ..Camera::default() });
    w.insert(b, extra(SHOT, serde_json::json!({ "duration": 2.0, "transition": "blend", "blend": 1.0 })));
    kit_scene::set_parent(w, b, director);
    let mut prev: Option<f32> = None;
    let mut biggest_step = 0.0f32;
    let mut mid = 0.0;
    for i in 0..100 {
        app.update(1.0 / 60.0);
        let cam = kit_gamesys::active_camera(&app.world).unwrap();
        let fov = fov_of(&app.world, cam).unwrap();
        if let Some(p) = prev {
            biggest_step = biggest_step.max((fov - p).abs());
        }
        prev = Some(fov);
        if i == 60 {
            mid = fov;
        }
    }
    assert!(biggest_step < 3.0, "the field of view jumped by {biggest_step}°");
    assert!(mid > 25.0 && mid < 55.0, "halfway through the blend: {mid}°");
    assert!((prev.unwrap() - 50.0).abs() < 0.01, "ends at B's own field of view");
}
