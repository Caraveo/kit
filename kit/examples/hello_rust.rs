//! Using Kit from Rust (no editor): `cargo run -p kit --example hello_rust`

use kit::prelude::*;

fn main() -> anyhow::Result<()> {
    let mut app = App::new();
    kit::engine::add_default_modules(&mut app, ".".into(), &Default::default());
    let cam = spawn_node(&mut app.world, "Camera", Transform::from_xyz(0.0, 1.0, 5.0));
    app.world.insert(cam, Camera::perspective());
    let cube = spawn_node(&mut app.world, "Cube", Transform::IDENTITY);
    app.world.insert(cube, Model::default());
    // A Rust system: spin the cube.
    app.add_play_system(Stage::Update, "spin", move |world| {
        let dt = world.resource::<Time>().delta;
        if let Some(mut t) = world.get_mut::<Transform>(cube) {
            t.rotation = Quat::from_rotation_y(dt) * t.rotation;
        }
    });
    run(app, EngineConfig { title: "Hello from Rust".into(), ..Default::default() })
}
