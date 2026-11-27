use super::*;
use kit_core::{Console, RunMode};
use kit_gamesys::GameSysModule;
use kit_input::{Input, InputModule};
use kit_resource::ResourceModule;
use kit_scene::{SceneModule, Transform};

fn temp_project(files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kit_script_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    for (p, src) in files {
        let path = dir.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, src).unwrap();
    }
    dir
}

fn app(root: std::path::PathBuf) -> App {
    let mut app = App::new();
    app.add_module(ResourceModule { root })
        .add_module(SceneModule)
        .add_module(InputModule)
        .add_module(GameSysModule)
        .add_module(ScriptModule);
    app
}

fn console_text(app: &App) -> Vec<String> {
    app.world.resource::<Console>().lines().iter().map(|l| l.text.clone()).collect()
}

#[test]
fn scripts_move_entities_and_read_input() {
    let root = temp_project(&[(
        "scripts/mover.x",
        r#"
TAKE Transform, Input, Time
CREATE Mover[
    FLOAT Speed: 2.0
    PUBLIC INTEGER Frames: 0
    VOID CREATE Init()[
        DISPLAY["hello from", Entity()]
    ]
    STRING CREATE Entity()[
        GIVE "mover"
    ]
    VOID CREATE Update(FLOAT dt)[
        Frames: Frames + 1
        Transform.Move(Speed * dt, 0, 0)
        IF(Input.Pressed("Space"))[
            Transform.Position: VECTOR(0, 10, 0)
        ]
    ]
    VOID CREATE Final()[
        DISPLAY["bye", Frames]
    ]
]
"#,
    )]);
    let mut app = app(root);
    let e = kit_scene::spawn_node(&mut app.world, "Mover", Transform::IDENTITY);
    app.world.insert(e, Script::new("scripts/mover.x"));

    // Frame steps are clamped to 0.25s by Time.
    app.update(0.25);
    app.update(0.25);
    assert!((app.world.get::<Transform>(e).unwrap().translation.x - 1.0).abs() < 1e-5);

    app.world.resource_mut::<Input>().press("Space");
    app.update(0.25);
    assert_eq!(app.world.get::<Transform>(e).unwrap().translation.y, 10.0);

    let fields = app.world.resource::<ScriptRuntime>().fields(e);
    assert!(fields.iter().any(|f| f.name == "Frames" && f.text == "3"), "{fields:?}");

    // Edit mode: scripts don't run.
    app.mode = RunMode::Edit;
    app.update(0.25);
    assert_eq!(app.world.get::<Transform>(e).unwrap().translation, glam::Vec3::new(0.0, 10.0, 0.0));

    stop_scripts(&mut app.world);
    let text = console_text(&app);
    assert_eq!(text, ["hello from mover", "bye 3"]);
}

#[test]
fn errors_go_to_console_and_stop_the_script() {
    let root = temp_project(&[
        ("bad_syntax.x", "CREATE Broken[\n  FLOAT A: \n]"),
        ("bad_runtime.x", "TAKE Transform\nVOID CREATE Update(FLOAT dt)[\n  Transform.Move(\"x\")\n]"),
    ]);
    let mut app = app(root);
    let a = kit_scene::spawn_node(&mut app.world, "A", Transform::IDENTITY);
    app.world.insert(a, Script::new("bad_syntax.x"));
    let b = kit_scene::spawn_node(&mut app.world, "B", Transform::IDENTITY);
    app.world.insert(b, Script::new("bad_runtime.x"));
    let c = kit_scene::spawn_node(&mut app.world, "C", Transform::IDENTITY);
    app.world.insert(c, Script::new("missing.x"));
    app.update(0.1);
    app.update(0.1);
    let text = console_text(&app);
    assert_eq!(text.len(), 3, "{text:?}");
    assert!(text.iter().any(|t| t.starts_with("bad_syntax.x:3")), "{text:?}");
    assert!(text.iter().any(|t| t.starts_with("bad_runtime.x:3") && t.contains("Transform.Move")), "{text:?}");
    assert!(text.iter().any(|t| t.starts_with("missing.x")), "{text:?}");
    assert!(app.world.resource::<ScriptRuntime>().has_error(b));
}

#[test]
fn function_scripts_scene_access_and_destroy() {
    let root = temp_project(&[
        ("lib/util.x", "FLOAT CREATE Double(FLOAT v)[ GIVE v * 2 ]"),
        (
            "boss.x",
            r#"
TAKE Scene, Entity
TAKE "lib/util.x"
INTEGER Count: 0
VOID CREATE Update(FLOAT dt)[
    Count: Count + 1
    ANY Minion: Scene.Find("Minion")
    IF(EXIST Minion)[
        Scene.SetPosition(Minion, VECTOR(Double(1.5), 0, 0))
    ]
    IF(Count == 2)[
        Scene.Destroy(Minion)
    ]
]
"#,
        ),
    ]);
    let mut app = app(root);
    let boss = kit_scene::spawn_node(&mut app.world, "Boss", Transform::IDENTITY);
    app.world.insert(boss, Script::new("boss.x"));
    let minion = kit_scene::spawn_node(&mut app.world, "Minion", Transform::IDENTITY);
    app.update(0.1);
    assert_eq!(app.world.get::<Transform>(minion).unwrap().translation.x, 3.0);
    app.update(0.1);
    assert!(!app.world.is_alive(minion), "{:?}", console_text(&app));
    app.update(0.1);
    assert!(console_text(&app).is_empty(), "{:?}", console_text(&app));
}

#[test]
fn terrain_height_from_scripts() {
    let root = temp_project(&[(
        "stand.x",
        r#"
TAKE Transform, Terrain
VOID CREATE Update(FLOAT dt)[
    ANY H: Terrain.HeightAt(Transform.Position.X, Transform.Position.Z)
    IF(EXIST H)[
        Transform.Position: VECTOR(Transform.Position.X, H + 0.5, Transform.Position.Z)
    ]
]
"#,
    )]);
    let mut app = app(root);
    let mut t = kit_gamesys::Terrain::flat(10.0, 11);
    t.flatten_all(2.0);
    let ground = kit_scene::spawn_node(&mut app.world, "Ground", Transform::from_xyz(0.0, 1.0, 0.0));
    app.world.insert(ground, t);
    let e = kit_scene::spawn_node(&mut app.world, "Walker", Transform::from_xyz(1.0, 20.0, 1.0));
    app.world.insert(e, Script::new("stand.x"));
    app.update(0.1);
    app.update(0.1);
    let y = app.world.get::<Transform>(e).unwrap().translation.y;
    assert!((y - 3.5).abs() < 1e-3, "stands on the terrain (1 + 2 + 0.5): {y}");
}

#[test]
fn free_camera_example_flies_and_looks() {
    let src = include_str!("../../../examples/hello/scripts/FreeCamera.x");
    let root = temp_project(&[("scripts/FreeCamera.x", src)]);
    let mut app = app(root);
    let e = kit_scene::spawn_node(&mut app.world, "Camera", Transform::IDENTITY);
    app.world.insert(e, Script::new("scripts/FreeCamera.x"));
    let step = |app: &mut App, frames: usize| {
        for _ in 0..frames {
            app.update(1.0 / 60.0);
        }
    };
    step(&mut app, 2);
    let errors: Vec<_> = console_text(&app).into_iter().filter(|l| !l.starts_with("FreeCamera:")).collect();
    assert!(errors.is_empty(), "{errors:?}");

    // W flies forward (-Z for an unrotated camera).
    app.world.resource_mut::<Input>().press("W");
    step(&mut app, 60);
    app.world.resource_mut::<Input>().release("W");
    let t = *app.world.get::<Transform>(e).unwrap();
    assert!(t.translation.z < -3.0 && t.translation.x.abs() < 1e-3, "{:?}", t.translation);

    // It glides to a stop after letting go.
    step(&mut app, 60);
    let stopped = app.world.get::<Transform>(e).unwrap().translation;
    step(&mut app, 10);
    assert!(app.world.get::<Transform>(e).unwrap().translation.distance(stopped) < 1e-3);

    // E lifts straight up; Shift is faster.
    let before = stopped.y;
    app.world.resource_mut::<Input>().press("E");
    step(&mut app, 30);
    let slow = app.world.get::<Transform>(e).unwrap().translation.y - before;
    assert!(slow > 0.5, "lifted {slow}");

    // Left arrow turns left: forward swings toward -X.
    app.world.resource_mut::<Input>().release("E");
    app.world.resource_mut::<Input>().press("Left");
    step(&mut app, 30);
    let fwd = app.world.get::<Transform>(e).unwrap().forward();
    assert!(fwd.x < -0.5 && fwd.y.abs() < 1e-3, "{fwd:?}");

    // Right-drag with the mouse looks up (mouse moving up the screen).
    app.world.resource_mut::<Input>().release("Left");
    app.world.resource_mut::<Input>().set_mouse(glam::Vec2::new(100.0, 100.0));
    app.update(1.0 / 60.0);
    app.world.resource_mut::<Input>().press("MouseRight");
    for i in 0..10 {
        app.world.resource_mut::<Input>().set_mouse(glam::Vec2::new(100.0, 100.0 - i as f32 * 20.0));
        app.update(1.0 / 60.0);
    }
    assert!(app.world.get::<Transform>(e).unwrap().forward().y > 0.3);
    let errors: Vec<_> = console_text(&app).into_iter().filter(|l| !l.starts_with("FreeCamera:")).collect();
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn ship_example_flies_turns_and_chases() {
    let src = include_str!("../../../examples/hello/scripts/Ship.x");
    let root = temp_project(&[("scripts/Ship.x", src)]);
    let mut app = app(root);
    // A second ship with the default setting: the Hello Kit model's nose is +Z,
    // so it flies toward +Z with the camera behind it on the -Z side.
    let mut nose = self::app(temp_project(&[("scripts/Ship.x", src)]));
    let s = kit_scene::spawn_node(&mut nose.world, "Ship", Transform::IDENTITY);
    nose.world.insert(s, Script::new("scripts/Ship.x"));
    let c = kit_scene::spawn_node(&mut nose.world, "Camera", Transform::IDENTITY);
    for _ in 0..30 {
        nose.update(1.0 / 60.0);
    }
    let ct = *nose.world.get::<Transform>(c).unwrap();
    assert!(ct.translation.z < -4.0 && ct.forward().z > 0.9, "camera behind a +Z nose: {:?}", ct.translation);
    nose.world.resource_mut::<Input>().press("W");
    for _ in 0..60 {
        nose.update(1.0 / 60.0);
    }
    assert!(nose.world.get::<Transform>(s).unwrap().translation.z > 5.0, "W flies toward the nose");

    // The rest uses Kit's own convention (nose along -Z).
    let ship = kit_scene::spawn_node(&mut app.world, "Ship", Transform::IDENTITY);
    let mut script = Script::new("scripts/Ship.x");
    script.values.insert("NoseIsPlusZ".into(), kit_gamesys::FieldValue::Bool(false));
    app.world.insert(ship, script);
    let cam = kit_scene::spawn_node(&mut app.world, "Camera", Transform::from_xyz(0.0, 50.0, 0.0));
    let step = |app: &mut App, frames: usize| {
        for _ in 0..frames {
            app.update(1.0 / 60.0);
        }
    };
    let errors = |app: &App| -> Vec<String> {
        console_text(app).into_iter().filter(|l| !l.starts_with("Ship:") && !l.starts_with("Flight assist")).collect()
    };
    let t = |app: &App, e| *app.world.get::<Transform>(e).unwrap();
    step(&mut app, 2);
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));

    // The chase camera sits behind (+Z) and above the ship and looks ahead of it.
    step(&mut app, 30);
    let c = t(&app, cam);
    assert!(c.translation.z > 4.0 && c.translation.y > 1.0, "{:?}", c.translation);
    assert!(c.forward().z < -0.9 && c.up().y > 0.9, "{:?} {:?}", c.forward(), c.up());

    // W thrusts along the nose (-Z); letting go with flight assist settles.
    app.world.resource_mut::<Input>().press("W");
    step(&mut app, 90);
    app.world.resource_mut::<Input>().release("W");
    let p = t(&app, ship).translation;
    assert!(p.z < -10.0 && p.x.abs() < 1e-3 && p.y.abs() < 1e-3, "{p:?}");
    step(&mut app, 120);
    let a = t(&app, ship).translation;
    step(&mut app, 10);
    assert!(t(&app, ship).translation.distance(a) < 1e-3, "assist stops the ship");

    // Up arrow pitches the nose up; Q rolls left (right wing rises).
    app.world.resource_mut::<Input>().press("Up");
    step(&mut app, 30);
    app.world.resource_mut::<Input>().release("Up");
    assert!(t(&app, ship).forward().y > 0.3, "{:?}", t(&app, ship).forward());
    step(&mut app, 60);
    let before = t(&app, ship);
    app.world.resource_mut::<Input>().press("Q");
    step(&mut app, 20);
    app.world.resource_mut::<Input>().release("Q");
    assert!(t(&app, ship).right().dot(before.up()) > 0.3, "rolled left");

    // Loop all the way over: the camera follows upside down without flipping.
    app.world.resource_mut::<Input>().press("Up");
    for _ in 0..240 {
        step(&mut app, 1);
        let (s, c) = (t(&app, ship), t(&app, cam));
        let to_ship = (s.translation - c.translation).normalize();
        assert!(c.forward().dot(to_ship) > 0.5, "camera keeps the ship in view");
        assert!(c.up().dot(s.up()) > 0.0, "camera banks with the ship");
    }
    app.world.resource_mut::<Input>().release("Up");

    // Flight assist off: the ship keeps drifting after thrust.
    app.world.resource_mut::<Input>().press("Z");
    step(&mut app, 1);
    app.world.resource_mut::<Input>().release("Z");
    app.world.resource_mut::<Input>().press("W");
    step(&mut app, 30);
    app.world.resource_mut::<Input>().release("W");
    step(&mut app, 60);
    let a = t(&app, ship).translation;
    step(&mut app, 30);
    assert!(t(&app, ship).translation.distance(a) > 1.0, "newtonian drift");

    // X brakes to a full stop.
    app.world.resource_mut::<Input>().press("X");
    step(&mut app, 120);
    let a = t(&app, ship).translation;
    step(&mut app, 10);
    assert!(t(&app, ship).translation.distance(a) < 1e-3, "brake stops the ship");
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
}

#[test]
fn scripts_control_audio_sources() {
    let root = temp_project(&[(
        "scripts/radio.x",
        r#"
TAKE Audio, Input
CREATE Radio[
    VOID CREATE Update(FLOAT dt)[
        IF(Input.Pressed("P"))[
            Audio.SetVolume(0.25)
            Audio.SetPitch(1.5)
            Audio.SetLoop(TRUE)
            Audio.Play()
            Audio.PlaySound("sounds/click.wav", 0.5)
        ]
        IF(Input.Pressed("S"))[
            Audio.Stop()
            DISPLAY["playing", Audio.Playing, "volume", Audio.Volume]
        ]
    ]
]
"#,
    )]);
    let mut app = app(root);
    let e = kit_scene::spawn_node(&mut app.world, "Radio", Transform::IDENTITY);
    app.world.insert(e, Script::new("scripts/radio.x"));
    app.world
        .insert(e, kit_gamesys::AudioSource { clip: "sounds/song.ogg".into(), spatial: true, ..Default::default() });
    app.update(0.1);
    app.world.resource_mut::<Input>().press("P");
    app.update(0.1);
    {
        let a = app.world.get::<kit_gamesys::AudioSource>(e).unwrap();
        assert_eq!((a.volume, a.pitch, a.looping), (0.25, 1.5, true));
        let cmds = &app.world.resource::<kit_gamesys::AudioCommands>().0;
        assert!(cmds.contains(&kit_gamesys::AudioCommand::Play(e)), "{cmds:?}");
        assert!(
            cmds.iter().any(|c| matches!(c, kit_gamesys::AudioCommand::OneShot { clip, volume, at: Some(_), .. }
                if clip == "sounds/click.wav" && *volume == 0.5)),
            "one-shots play from the object with its proximity: {cmds:?}"
        );
    }
    app.world.resource_mut::<Input>().release("P");
    app.world.resource_mut::<Input>().press("S");
    app.update(0.1);
    assert!(app.world.resource::<kit_gamesys::AudioCommands>().0.contains(&kit_gamesys::AudioCommand::Stop(e)));
    assert!(console_text(&app).iter().any(|l| l == "playing FALSE volume 0.25"), "{:?}", console_text(&app));
}

#[test]
fn edit_values_apply_before_init_and_private_fields_stay_hidden() {
    let root = temp_project(&[(
        "mover.x",
        r#"
TAKE Transform
CREATE Mover[
    EDIT FLOAT Speed: 1.0
    EDIT VECTOR Start: VECTOR(0, 0, 0)
    PUBLIC INTEGER Moves: 0
    FLOAT Hidden: 3.0
    VOID CREATE Init()[
        Transform.Position: Start
    ]
    VOID CREATE Update(FLOAT dt)[
        Transform.Position: Transform.Position + VECTOR(Speed * dt, 0, 0)
        Moves: Moves + 1
    ]
]
"#,
    )]);
    let mut app = app(root.clone());
    let e = kit_scene::spawn_node(&mut app.world, "M", Transform::IDENTITY);
    let mut s = Script::new("mover.x");
    s.values.insert("Speed".into(), kit_gamesys::FieldValue::Float(10.0));
    s.values.insert("Start".into(), kit_gamesys::FieldValue::Vector([0.0, 5.0, 0.0]));
    s.values.insert("Moves".into(), kit_gamesys::FieldValue::Int(99)); // PUBLIC: not settable
    app.world.insert(e, s);
    app.update(0.5);
    let p = app.world.get::<Transform>(e).unwrap().translation;
    assert!(
        (p.y - 5.0).abs() < 1e-4 && p.x > 2.0,
        "Init saw Start, Update used Speed 10 (the default 1 would move < 0.5): {p:?}"
    );
    assert!(console_text(&app).iter().any(|l| l.contains("`Moves`") && l.contains("isn't an EDIT field")));

    let mut rt = app.world.resource_mut::<crate::ScriptRuntime>();
    let names: Vec<(String, crate::Access)> = rt.fields(e).into_iter().map(|f| (f.name, f.access)).collect();
    assert_eq!(
        names,
        [
            ("Speed".into(), crate::Access::Edit),
            ("Start".into(), crate::Access::Edit),
            ("Moves".into(), crate::Access::Public)
        ]
    );
    assert!(rt.set_field(e, "Speed", &kit_gamesys::FieldValue::Float(2.0)));
    assert!(!rt.set_field(e, "Moves", &kit_gamesys::FieldValue::Int(1)), "PUBLIC is read-only");
    assert!(!rt.set_field(e, "Hidden", &kit_gamesys::FieldValue::Float(1.0)), "private");
    // Declared fields (editor, before Play) match.
    let res = kit_resource::Resources::new(root);
    let d = crate::declared_fields(&res, "mover.x");
    assert_eq!(d.fields.len(), 3);
    assert_eq!(d.fields[0].value, Some(kit_gamesys::FieldValue::Float(1.0)));
}

#[test]
fn scripts_switch_scenes_at_runtime() {
    let root = temp_project(&[
        ("scenes/level2.kscene", r#"{"version":1,"entities":[{"id":1,"name":"Cave"}]}"#),
        ("scenes/sky.kscene", r#"{"version":1,"entities":[{"id":1,"name":"Clouds"}]}"#),
        (
            "door.x",
            r#"
TAKE Scene
INTEGER Count: 0
VOID CREATE Update(FLOAT dt)[
    Count: Count + 1
    IF(Count == 1)[ Scene.Load("scenes/missing") ]
    IF(Count == 2)[ Scene.Load("scenes/level2") ]
]
"#,
        ),
        (
            "cave.x",
            r#"
TAKE Scene
INTEGER Count: 0
VOID CREATE Update(FLOAT dt)[
    Count: Count + 1
    IF(Count == 1)[ Scene.LoadAdditive("scenes/sky") ]
    IF(Count == 2)[ Scene.Unload("scenes/sky") ]
]
"#,
        ),
    ]);
    let mut app = app(root);
    let door = kit_scene::spawn_node(&mut app.world, "Door", Transform::IDENTITY);
    app.world.insert(door, Script::new("door.x"));
    kit_gamesys::scenes::mark_loaded(&mut app.world, "main.kscene", vec![door]);

    // A missing scene leaves the current one running.
    app.update(0.1);
    assert!(app.world.is_alive(door));
    assert!(matches!(&app.world.resource::<kit_gamesys::scenes::SceneEvents>().0[..], [kit_gamesys::scenes::SceneEvent::Failed { .. }]));

    app.update(0.1);
    assert!(!app.world.is_alive(door), "the old scene is gone");
    let cave = kit_scene::find_by_name(&app.world, "Cave").expect("level2 loaded");
    assert_eq!(app.world.resource::<kit_gamesys::scenes::LoadedScenes>().current(), Some("scenes/level2.kscene"));

    app.world.insert(cave, Script::new("cave.x"));
    app.update(0.1);
    assert!(kit_scene::find_by_name(&app.world, "Clouds").is_some(), "additive scene on top");
    assert!(app.world.is_alive(cave));
    app.update(0.1);
    assert!(kit_scene::find_by_name(&app.world, "Clouds").is_none(), "additive scene removed");
    assert!(app.world.is_alive(cave), "main scene stays");
}

#[test]
fn invoke_spawns_shapes_models_and_copies() {
    let root = temp_project(&[
        (
            "scripts/field.x",
            r#"
TAKE Scene, Entity
CREATE Field[
    PUBLIC INTEGER Made: 0
    VOID CREATE Init()[
        ANY Ball: INVOKE("sphere", VECTOR(1, 2, 3), VECTOR(0, 90, 0), 2)
        INVOKE("models/rock/Rock.kmodel", VECTOR(0, 0, -5))
        # A copy of the scripted Template, by name and by id.
        ANY A: INVOKE("Template", VECTOR(10, 0, 0))
        ANY B: INVOKE(A, VECTOR(20, 0, 0), VECTOR(0, 0, 0), VECTOR(1, 2, 3))
        # INSTANTIATE: the same, but at the top level, in world space.
        INSTANTIATE("Torus", VECTOR(0, 0, 3))
        INSTANTIATE("Template", VECTOR(30, 0, 0))
        Made: 4
    ]
]
"#,
        ),
        (
            "scripts/spin.x",
            r#"
TAKE Transform
CREATE Spin[
    VOID CREATE Update(FLOAT dt)[
        Transform.Rotate(0, 90 * dt, 0)
    ]
]
"#,
        ),
        ("scripts/bad.x", "CREATE Bad[\n    VOID CREATE Init()[\n        INVOKE(\"Nope\")\n    ]\n]\n"),
    ]);
    let mut app = app(root);
    let template = kit_scene::spawn_node(&mut app.world, "Template", Transform::from_xyz(0.0, 7.0, 0.0));
    app.world.insert(template, kit_gamesys::Model::default());
    app.world.insert(template, Script::new("scripts/spin.x"));
    let field = kit_scene::spawn_node(&mut app.world, "Field", Transform::from_xyz(100.0, 0.0, 0.0));
    app.world.insert(field, Script::new("scripts/field.x"));
    for _ in 0..10 {
        app.update(1.0 / 60.0);
    }
    assert!(console_text(&app).is_empty(), "{:?}", console_text(&app));

    // Spawned shapes are named after the shape.
    let w = &app.world;
    let named = |n: &str| -> Vec<kit_ecs::Entity> {
        w.storage::<kit_scene::Name>().unwrap().iter().filter(|(_, x)| x.0 == n).map(|(e, _)| e).collect()
    };
    let ball = named("Sphere");
    assert_eq!(ball.len(), 1);
    let t = *w.get::<Transform>(ball[0]).unwrap();
    assert!(t.translation.distance(glam::Vec3::new(1.0, 2.0, 3.0)) < 1e-5 && (t.scale.x - 2.0).abs() < 1e-5);
    assert!(t.forward().distance(glam::Vec3::NEG_X) < 1e-4, "rotated 90 degrees: {:?}", t.forward());
    assert_eq!(w.get::<kit_gamesys::Model>(ball[0]).unwrap().mesh, kit_gamesys::MeshKind::Sphere);
    // The invoking object is the parent, so the position is relative to it.
    assert_eq!(w.get::<kit_scene::Parent>(ball[0]).map(|p| p.0), Some(field));
    assert!(w.get::<kit_scene::RootOrder>(ball[0]).is_none());
    let world_x = w.get::<kit_scene::GlobalTransform>(ball[0]).unwrap().0.w_axis.x;
    assert!((world_x - 101.0).abs() < 1e-4, "{world_x}");
    let top = named("Torus")[0];
    assert!(w.get::<kit_scene::Parent>(top).is_none() && w.get::<kit_scene::RootOrder>(top).is_some(), "top level");
    assert_eq!(w.get::<kit_scene::GlobalTransform>(top).unwrap().0.w_axis.z, 3.0, "world space");

    let rock = named("Rock");
    assert_eq!(w.get::<kit_gamesys::Model>(rock[0]).unwrap().mesh_asset, "models/rock/Rock.kmodel");

    // Copies keep the template's name, components and script, and their scripts run.
    let copies = named("Template");
    assert_eq!(copies.len(), 4);
    assert_eq!(copies.iter().filter(|&&c| w.get::<kit_scene::Parent>(c).map(|p| p.0) == Some(field)).count(), 2);
    let top_copy = copies.iter().copied().find(|&c| w.get::<Transform>(c).unwrap().translation.x == 30.0).unwrap();
    assert!(w.get::<kit_scene::Parent>(top_copy).is_none() && w.get::<kit_scene::RootOrder>(top_copy).is_some(), "instantiated copy is top level");
    for &c in &copies {
        assert!(w.get::<kit_gamesys::Model>(c).is_some() && w.get::<Script>(c).is_some());
        assert!(w.get::<Transform>(c).unwrap().forward().x.abs() > 0.1, "each copy spins on its own");
    }
    let b = copies.iter().find(|&&c| w.get::<Transform>(c).unwrap().translation.x == 20.0).unwrap();
    assert_eq!(w.get::<Transform>(*b).unwrap().scale, glam::Vec3::new(1.0, 2.0, 3.0));

    // A name that isn't a shape, a model or an object is a clear error.
    let bad = kit_scene::spawn_node(&mut app.world, "Bad", Transform::IDENTITY);
    app.world.insert(bad, Script::new("scripts/bad.x"));
    app.update(1.0 / 60.0);
    let text = console_text(&app).join("\n");
    assert!(text.contains("INVOKE: nothing called \"Nope\"") && text.contains("Sphere"), "{text}");
}

fn run_frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update(1.0 / 60.0);
    }
}

fn int_field(app: &App, e: kit_ecs::Entity, name: &str) -> String {
    app.world.resource::<ScriptRuntime>().fields(e).into_iter().find(|f| f.name == name).map(|f| f.text).unwrap_or_default()
}

#[test]
fn features_let_scripts_reach_each_other_and_engine_parts() {
    let root = temp_project(&[
        (
            "scripts/Enemy.x",
            r#"
CREATE Enemy[
    PUBLIC INTEGER Health: 100
    PUBLIC INTEGER Hits: 0
    INTEGER CREATE TakeDamage(INTEGER amount)[
        Health: Health - amount
        Hits: Hits + 1
        GIVE Health
    ]
]
"#,
        ),
        (
            "scripts/Player.x",
            r#"
TAKE Scene
CREATE Player[
    PUBLIC INTEGER Seen: 0
    PUBLIC STRING Report: ""
    VOID CREATE Update(FLOAT dt)[
        IF(Seen == 0)[
            ANY Foe: Scene.Find("Foe")
            Enemy E: GetFeature(Foe, "Enemy")
            Seen: E.TakeDamage(30)
            E.Health: E.Health - 5
            # Engine features on another object.
            ANY M: GetFeature(Foe, "Model")
            M.SetColor(0.1, 0.2, 0.3)
            M.Enabled: FALSE
            Report: STRING(HasFeature(Foe, "Model")) + " " + STRING(HasFeature(Foe, "Physics")) + " " + STRING(GetFeature(Foe, "Light") == NULL)
            # Add and remove features at runtime.
            ANY L: AddFeature(Foe, "Light")
            L.SetBrightness(3)
            RemoveFeature(Foe, "Model")
        ]
    ]
]
"#,
        ),
    ]);
    let mut app = app(root);
    let foe = kit_scene::spawn_node(&mut app.world, "Foe", Transform::IDENTITY);
    app.world.insert(foe, kit_gamesys::Model::default());
    app.world.insert(foe, Script::new("scripts/Enemy.x"));
    // The player comes first in the world, so the enemy's script must be ready before any Update.
    let player = kit_scene::spawn_node(&mut app.world, "Player", Transform::IDENTITY);
    app.world.insert(player, Script::new("scripts/Player.x"));
    run_frames(&mut app, 1);
    assert!(console_text(&app).is_empty(), "{:?}", console_text(&app));
    assert_eq!(int_field(&app, player, "Seen"), "70", "called a method on the other script");
    assert_eq!(int_field(&app, foe, "Health"), "65", "set a field on the other script");
    assert_eq!(int_field(&app, foe, "Hits"), "1");
    assert_eq!(int_field(&app, player, "Report"), "TRUE FALSE TRUE");
    assert!(!app.world.has::<kit_gamesys::Model>(foe), "RemoveFeature");
    assert_eq!(app.world.get::<kit_gamesys::Light>(foe).unwrap().intensity, 3.0, "AddFeature + use it");
}

#[test]
fn be_active_timers_coroutines_and_late_update() {
    let root = temp_project(&[(
        "scripts/Clock.x",
        r#"
TAKE Time, Transform, Scene
CREATE Clock[
    PUBLIC INTEGER Ticks: 0
    PUBLIC INTEGER Later: 0
    PUBLIC INTEGER Steps: 0
    PUBLIC INTEGER Fixed: 0
    PUBLIC FLOAT LateX: 0
    PUBLIC INTEGER Repeat: 0
    VOID CREATE Init()[
        Time.After(0.5, "Ring", 7)
        Time.Every(0.25, "Tick")
        ANY Never: Time.After(0.3, "Ring", 1000)
        Time.Cancel(Never)
        Time.Start("Walk", 3)
    ]
    VOID CREATE Ring(INTEGER n)[
        Later: Later + n
    ]
    VOID CREATE Tick()[
        Repeat: Repeat + 1
    ]
    VOID CREATE Walk(INTEGER count)[
        GRAB I IN RANGE(count)[
            Steps: Steps + 1
            WAIT 0.2
        ]
        LOOP(Steps < 10)[
            Steps: Steps + 1
            IF(Steps == 5)[
                EXIT
            ]
            WAIT 0
        ]
    ]
    VOID CREATE FixedUpdate(FLOAT step)[
        Fixed: Fixed + 1
    ]
    VOID CREATE Update(FLOAT dt)[
        Ticks: Ticks + 1
        Transform.Move(1, 0, 0)
    ]
    VOID CREATE LateUpdate(FLOAT dt)[
        LateX: Transform.WorldPosition.X
    ]
]
"#,
    )]);
    let mut app = app(root);
    let e = kit_scene::spawn_node(&mut app.world, "Clock", Transform::IDENTITY);
    app.world.insert(e, Script::new("scripts/Clock.x"));
    run_frames(&mut app, 1);
    assert_eq!(int_field(&app, e, "LateX"), "1.0", "LateUpdate sees this frame's movement");
    run_frames(&mut app, 62); // a little over a second (timers started at the first frame)
    assert!(console_text(&app).is_empty(), "{:?}", console_text(&app));
    assert_eq!(int_field(&app, e, "Later"), "7", "After fired once; the cancelled one never did");
    assert_eq!(int_field(&app, e, "Repeat"), "4", "Every 0.25 s for a second");
    assert_eq!(int_field(&app, e, "Steps"), "5", "coroutine waited, looped and exited");
    let fixed: i64 = int_field(&app, e, "Fixed").parse().unwrap();
    assert!((62..=64).contains(&fixed), "FixedUpdate ~60 times a second: {fixed}");

    // Switched off: no updates, not drawn; back on: carries on.
    let before = int_field(&app, e, "Ticks");
    app.world.insert(e, kit_scene::Inactive);
    run_frames(&mut app, 5);
    assert_eq!(int_field(&app, e, "Ticks"), before);
    app.world.remove::<kit_scene::Inactive>(e);
    run_frames(&mut app, 1);
    assert_ne!(int_field(&app, e, "Ticks"), before);
}

#[test]
fn rotation_math_matches_the_engine() {
    let root = temp_project(&[(
        "scripts/Rot.x",
        r#"
TAKE Transform, Rotation, Math
CREATE Rot[
    PUBLIC VECTOR Mid: VECTOR(0, 0, 0)
    PUBLIC VECTOR Fwd: VECTOR(0, 0, 0)
    PUBLIC VECTOR Look: VECTOR(0, 0, 0)
    PUBLIC VECTOR Turned: VECTOR(0, 0, 0)
    PUBLIC FLOAT Angle: 0
    PUBLIC VECTOR Moved: VECTOR(0, 0, 0)
    PUBLIC FLOAT Remapped: 0
    PUBLIC VECTOR World: VECTOR(0, 0, 0)
    PUBLIC VECTOR Back: VECTOR(0, 0, 0)
    VOID CREATE Init()[
        Mid: Rotation.Blend(VECTOR(0, 0, 0), VECTOR(0, 90, 0), 0.5)
        Fwd: Rotation.Forward(VECTOR(0, 90, 0))
        Look: Rotation.LookToward(VECTOR(1, 0, 0))
        Turned: Rotation.Apply(Rotation.AroundAxis(VECTOR(0, 0, 1), 90), VECTOR(1, 0, 0))
        Angle: Rotation.Angle(VECTOR(0, 0, 0), Rotation.Combine(VECTOR(0, 30, 0), VECTOR(0, 15, 0)))
        Moved: Math.MoveToward(VECTOR(0, 0, 0), VECTOR(10, 0, 0), 3)
        Remapped: Math.Remap(5, 0, 10, 100, 200)
        Transform.Position: VECTOR(1, 2, 3)
        Transform.Rotation: VECTOR(0, 90, 0)
        World: Transform.ToWorld(VECTOR(0, 0, -1))
        Back: Transform.ToLocal(World)
    ]
]
"#,
    )]);
    let mut app = app(root);
    let e = kit_scene::spawn_node(&mut app.world, "Rot", Transform::IDENTITY);
    app.world.insert(e, Script::new("scripts/Rot.x"));
    run_frames(&mut app, 2);
    assert!(console_text(&app).is_empty(), "{:?}", console_text(&app));
    let v = |n: &str| -> glam::Vec3 {
        match app.world.resource::<ScriptRuntime>().fields(e).into_iter().find(|f| f.name == n).and_then(|f| f.value) {
            Some(kit_gamesys::FieldValue::Vector(v)) => glam::Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32),
            Some(kit_gamesys::FieldValue::Float(f)) => glam::Vec3::splat(f as f32),
            other => panic!("{n}: {other:?}"),
        }
    };
    let close = |a: glam::Vec3, b: glam::Vec3| a.distance(b) < 1e-3;
    assert!(close(v("Mid"), glam::Vec3::new(0.0, 45.0, 0.0)), "{:?}", v("Mid"));
    // Same convention as the engine: a 90° Y turn faces -X.
    let engine = Transform { rotation: glam::Quat::from_rotation_y(90f32.to_radians()), ..Transform::IDENTITY }.forward();
    assert!(close(v("Fwd"), engine), "{:?} vs {engine:?}", v("Fwd"));
    let mut t = Transform::IDENTITY;
    t.set_euler_degrees(v("Look"));
    assert!(close(t.forward(), glam::Vec3::X), "LookToward faces +X: {:?}", t.forward());
    assert!(close(v("Turned"), glam::Vec3::Y));
    assert!((v("Angle").x - 45.0).abs() < 1e-3);
    assert!(close(v("Moved"), glam::Vec3::new(3.0, 0.0, 0.0)));
    assert!((v("Remapped").x - 150.0).abs() < 1e-6);
    assert!(close(v("World"), glam::Vec3::new(0.0, 2.0, 3.0)), "{:?}", v("World"));
    assert!(close(v("Back"), glam::Vec3::new(0.0, 0.0, -1.0)));
}

#[test]
fn diagrams_run_script_functions_and_hear_script_events() {
    let root = temp_project(&[(
        "scripts/Door.x",
        r#"
TAKE Diagram
CREATE Door[
    INTEGER CREATE Open(INTEGER speed)[
        Diagram.Emit("DoorOpened", speed * 2)
        GIVE speed + 1
    ]
]
"#,
    )]);
    let mut app = app(root);
    let e = kit_scene::spawn_node(&mut app.world, "Door", Transform::IDENTITY);
    app.world.insert(e, Script::new("scripts/Door.x"));
    run_frames(&mut app, 1);
    app.world.resource_mut::<kit_gamesys::ScriptCalls>().pending.push(kit_gamesys::ScriptCall {
        id: 9,
        target: e,
        function: "Open".into(),
        args: vec![kit_gamesys::BridgeValue::Int(4)],
    });
    app.world.resource_mut::<kit_gamesys::ScriptCalls>().pending.push(kit_gamesys::ScriptCall {
        id: 10,
        target: e,
        function: "Missing".into(),
        args: vec![],
    });
    // Look at the queues right after the scripts ran (before graphs would take them).
    run_scripts(&mut app.world);
    let results = std::mem::take(&mut app.world.resource_mut::<kit_gamesys::ScriptCalls>().results);
    assert_eq!(results[0], (9, Ok(Some(kit_gamesys::BridgeValue::Int(5)))));
    assert!(matches!(&results[1], (10, Err(m)) if m.contains("no function `Missing`")), "{results:?}");
    let events = std::mem::take(&mut app.world.resource_mut::<kit_gamesys::GraphEvents>().0);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "DoorOpened");
    assert_eq!(events[0].payload, Some(kit_gamesys::BridgeValue::Int(8)));
}

#[test]
fn tags_saves_animation_and_debug_lines() {
    let save = std::env::temp_dir().join(format!("kit_save_test_{}.json", std::process::id()));
    let _ = std::fs::remove_file(&save);
    // SAFETY: only this test reads KIT_SAVE_FILE.
    unsafe { std::env::set_var("KIT_SAVE_FILE", &save) };
    let root = temp_project(&[(
        "scripts/Misc.x",
        r#"
TAKE Entity, Scene, Save, Model, Debug
CREATE Misc[
    PUBLIC INTEGER Tagged: 0
    PUBLIC INTEGER Loaded: 0
    VOID CREATE Init()[
        Entity.AddTag("Enemy")
        Tagged: LENGTH(Scene.FindTagged("enemy"))
        Save.Set("best", 42)
        Save.Set("spot", VECTOR(1, 2, 3))
        Loaded: Save.Get("best", 0) + Save.Get("missing", 1)
        Model.Play("Run")
        Debug.Line(VECTOR(0, 0, 0), VECTOR(1, 1, 1))
        Debug.Sphere(VECTOR(0, 0, 0), 2, VECTOR(1, 0, 0), 5)
    ]
]
"#,
    )]);
    let mut app = app(root);
    let e = kit_scene::spawn_node(&mut app.world, "Misc", Transform::IDENTITY);
    app.world.insert(e, kit_gamesys::Model::default());
    app.world.insert(e, Script::new("scripts/Misc.x"));
    app.update(1.0 / 60.0);
    app.update(1.0 / 60.0);
    assert!(console_text(&app).is_empty(), "{:?}", console_text(&app));
    assert_eq!(int_field(&app, e, "Tagged"), "1");
    assert_eq!(int_field(&app, e, "Loaded"), "43");
    let saved = std::fs::read_to_string(&save).unwrap();
    assert!(saved.contains("\"best\": 42") && saved.contains("$vector"), "{saved}");
    let m = app.world.get::<kit_gamesys::Model>(e).unwrap().clone();
    assert_eq!(m.animation, "Run");
    assert!(m.clock.is_some_and(|c| c > 0.0 && c < 0.1), "its own clock: {:?}", m.clock);
    // The one-frame line is gone after the second frame; the 5-second sphere stays.
    assert_eq!(app.world.resource::<kit_gamesys::DebugDraw>().lines.len(), 72);
    let _ = std::fs::remove_file(&save);
}
