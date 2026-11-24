//! Runs the bundled example project headless (no window) and checks that every
//! script loads and runs without errors.

use kit_core::{Console, LogLevel};
use kit_input::Input;
use kit_resource::Project;
use std::path::Path;

#[test]
fn hello_example_runs_clean() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let project = Project::open(&root).expect("open example");
    let mut app = kit_engine::project_app(&project).expect("load main scene");
    assert!(app.world.entity_count() > 0);

    // The Player script (when the example still has it; it's meant to be edited).
    let player = kit_scene::find_by_name(&app.world, "Player");
    let start = player.and_then(|p| app.world.get::<kit_scene::Transform>(p).map(|t| t.translation));
    app.world.resource_mut::<Input>().press("KeyD");
    for frame in 0..120 {
        if frame == 60 {
            app.world.resource_mut::<Input>().press("Space");
        }
        app.update(1.0 / 60.0);
    }
    if let (Some(player), Some(start)) = (player, start) {
        let end = app.world.get::<kit_scene::Transform>(player).unwrap().translation;
        assert!(end.x > start.x + 1.0, "player should move right: {start} -> {end}");
    }

    // Physics and paths (when those objects exist; the example is meant to be edited).
    if let Some(ball) = kit_scene::find_by_name(&app.world, "Ball 1") {
        let y = app.world.get::<kit_scene::Transform>(ball).unwrap().translation.y;
        assert!(y < 2.4, "ball fell: y = {y}");
    }
    if let Some(rover) = kit_scene::find_by_name(&app.world, "Rover") {
        if app.world.has::<kit_gamesys::PathFollow>(rover) {
            let r = app.world.get::<kit_scene::Transform>(rover).unwrap().translation;
            assert!(r.z < 2.0, "rover moved along its path: {r}");
        }
    }

    let console = app.world.resource::<Console>();
    let errors: Vec<_> =
        console.lines().iter().filter(|l| l.level == LogLevel::Error).map(|l| l.text.clone()).collect();
    assert!(errors.is_empty(), "script errors: {errors:#?}");
    let text: Vec<_> = console.lines().iter().map(|l| l.text.clone()).collect();
    if player.is_some() {
        assert!(text.iter().any(|t| t.starts_with("Hello from x!")), "{text:?}");
        assert!(text.iter().any(|t| t.starts_with("Color 1 after")), "{text:?}");
    }
}
