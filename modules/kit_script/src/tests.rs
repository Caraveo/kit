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
