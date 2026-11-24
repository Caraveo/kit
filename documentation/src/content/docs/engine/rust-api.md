---
title: Using Kit from Rust
description: Build games or tools directly against the engine crates.
---

Add the `kit` crate and use the prelude:

```rust
use kit::prelude::*;

fn main() -> anyhow::Result<()> {
    let mut app = App::new();
    kit::engine::add_default_modules(&mut app, ".".into(), &Default::default());

    let cam = spawn_node(&mut app.world, "Camera", Transform::from_xyz(0.0, 1.0, 5.0));
    app.world.insert(cam, Camera::perspective());

    let cube = spawn_node(&mut app.world, "Cube", Transform::IDENTITY);
    app.world.insert(cube, Model::default());
    app.world.insert(cube, Collider::default());
    app.world.insert(cube, RigidBody::default());

    // A Rust system.
    app.add_play_system(Stage::Update, "spin", move |world| {
        let dt = world.resource::<Time>().delta;
        if let Some(mut t) = world.get_mut::<Transform>(cube) {
            t.rotation = Quat::from_rotation_y(dt) * t.rotation;
        }
    });

    run(app, EngineConfig { title: "Hello from Rust".into(), ..Default::default() })
}
```

## The world

```rust
let e = world.spawn();
world.insert(e, Name("Thing".into()));
for e in world.query::<(Transform, Model)>() {
    let mut t = world.get_mut::<Transform>(e).unwrap();
    t.translation.y += 1.0;
}
world.insert_resource(MyScore(0));
*world.resource_mut::<MyScore>() = MyScore(10);
```

## Writing a module

```rust
struct Gravity2x;
impl Module for Gravity2x {
    fn name(&self) -> &'static str { "gravity2x" }
    fn dependencies(&self) -> &'static [&'static str] { &["physics"] }
    fn build(&self, app: &mut App) {
        app.world.resource_mut::<Physics>().0.set_gravity(Vec3::new(0.0, -19.6, 0.0));
    }
}
```

API docs: run `cargo doc --open` in the Kit repository.
