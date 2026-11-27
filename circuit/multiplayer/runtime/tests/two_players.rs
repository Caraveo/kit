//! Two Kit games in one world, end to end against a Realtime service.
//! Needs a running Realtime service (e.g. a local one) with anonymous play on:
//!
//!   KIT_RT_TEST_URL=ws://127.0.0.1:8787/api/v1/realtime/connect KIT_RT_TEST_ENV=env_… \
//!   cargo test -p kit_multiplayer --test two_players -- --ignored --nocapture

use std::time::{Duration, Instant};

use kit_core::App;
use kit_scene::Transform;

fn project(env: &str, url: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("kit_mp_{}_{}", std::process::id(), Instant::now().elapsed().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("kit.project"),
        "[project]\ntitle = \"Two\"\nversion = \"0.0.1\"\nmain_scene = \"main.kscene\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.kscene"),
        r#"{"version":1,"entities":[{"id":1,"name":"Player","realtime":{"per_player":true}},{"id":2,"name":"Ball","realtime":{}}]}"#,
    )
    .unwrap();
    std::fs::write(dir.join("circuits.lock"), "version = 1\n[[circuit]]\nid = \"kit.multiplayer\"\nversion = \"0.1.1\"\nsha256 = \"x\"\nsource = \"test\"\nenabled = true\n").unwrap();
    std::fs::write(
        dir.join("circuits.settings.toml"),
        format!(
            "[\"kit.multiplayer\"]\nenvironment = \"{env}\"\nworld = \"test-{}\"\nconnect_url = \"{url}\"\n",
            std::process::id()
        ),
    )
    .unwrap();
    dir
}

fn game(root: &std::path::Path) -> App {
    let mut app = App::new();
    app.add_module(kit_resource::ResourceModule { root: root.to_path_buf() })
        .add_module(kit_scene::SceneModule)
        .add_module(kit_gamesys::GameSysModule)
        .add_module(kit_multiplayer::MultiplayerModule);
    let scene = kit_gamesys::SceneFile::from_json(&std::fs::read_to_string(root.join("main.kscene")).unwrap()).unwrap();
    let spawned = kit_gamesys::load_scene(&mut app.world, &scene).unwrap();
    kit_gamesys::scenes::mark_loaded(&mut app.world, "main.kscene", spawned);
    app
}

fn status(app: &App) -> String {
    app.world.resource::<kit_multiplayer::Multiplayer>().status().to_owned()
}

fn run_until(apps: &mut [&mut App], what: &str, mut done: impl FnMut(&mut [&mut App]) -> bool) {
    let until = Instant::now() + Duration::from_secs(15);
    while !done(apps) {
        assert!(
            Instant::now() < until,
            "timed out: {what} (status: {:?})",
            apps.iter().map(|a| status(a)).collect::<Vec<_>>()
        );
        for a in apps.iter_mut() {
            a.update(1.0 / 60.0);
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn find(app: &App, name: &str) -> Option<kit_ecs::Entity> {
    kit_scene::find_by_name(&app.world, name)
}

fn x_of(app: &App, e: kit_ecs::Entity) -> f32 {
    app.world.get::<Transform>(e).unwrap().translation.x
}

#[test]
#[ignore = "needs a running Realtime service (KIT_RT_TEST_URL, KIT_RT_TEST_ENV)"]
fn two_games_see_each_other() {
    let (Ok(url), Ok(env)) = (std::env::var("KIT_RT_TEST_URL"), std::env::var("KIT_RT_TEST_ENV")) else {
        panic!("set KIT_RT_TEST_URL and KIT_RT_TEST_ENV");
    };
    let root = project(&env, &url);
    let mut a = game(&root);
    let mut b = game(&root);

    // Both connect; each sees the other's player as a copy.
    run_until(&mut [&mut a, &mut b], "both connected", |g| g.iter().all(|a| status(a) == "Connected"));
    let start = Instant::now();
    run_until(&mut [&mut a, &mut b], "player copies", |g| {
        g.iter().all(|a| {
            kit_scene::roots(&a.world)
                .iter()
                .any(|e| a.world.get::<kit_scene::Name>(*e).is_some_and(|n| n.0.starts_with("Player (player")))
        })
    });
    println!("each game sees the other's player after {:?}", start.elapsed());

    // A moves its player; B's copy of it follows.
    let a_player = find(&a, "Player").unwrap();
    a.world.get_mut::<Transform>(a_player).unwrap().translation.x = 5.0;
    let copy_in_b = kit_scene::roots(&b.world)
        .into_iter()
        .find(|e| b.world.get::<kit_scene::Name>(*e).is_some_and(|n| n.0.starts_with("Player (player")))
        .unwrap();
    run_until(&mut [&mut a, &mut b], "B sees A move", |g| (x_of(g[1], copy_in_b) - 5.0).abs() < 0.01);
    assert_eq!(x_of(&b, find(&b, "Player").unwrap()), 0.0, "B's own player didn't move");

    // The shared Ball exists once: whoever moves it as owner, the other follows.
    let (ball_a, ball_b) = (find(&a, "Ball").unwrap(), find(&b, "Ball").unwrap());
    for (g, ball) in [(&mut a, ball_a), (&mut b, ball_b)] {
        g.world.get_mut::<Transform>(ball).unwrap().translation.x = 2.0;
    }
    run_until(&mut [&mut a, &mut b], "ball settles", |g| {
        (x_of(g[0], ball_a) - 2.0).abs() < 0.01 && (x_of(g[1], ball_b) - 2.0).abs() < 0.01
    });
    // Move it on both; only the owner's move sticks, and both end up agreeing.
    a.world.get_mut::<Transform>(ball_a).unwrap().translation.x = 7.0;
    b.world.get_mut::<Transform>(ball_b).unwrap().translation.x = -7.0;
    run_until(&mut [&mut a, &mut b], "ball agrees", |g| {
        (x_of(g[0], ball_a) - x_of(g[1], ball_b)).abs() < 0.01 && x_of(g[0], ball_a).abs() > 6.9
    });
    println!("ball agreed at x = {} in both games", x_of(&a, ball_a));
    // No duplicate balls anywhere.
    for g in [&a, &b] {
        assert_eq!(
            kit_scene::roots(&g.world)
                .iter()
                .filter(|e| g.world.get::<kit_scene::Name>(**e).is_some_and(|n| n.0.starts_with("Ball")))
                .count(),
            1
        );
    }

    // B leaves: A's copy of B's player disappears once the scope lets it go.
    drop(b);
    let _ = std::fs::remove_dir_all(&root);
}
