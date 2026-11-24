//! # Kit
//!
//! A modular game engine: Rust, Vulkan, an ECS with a scene graph, and the
//! **x** scripting language. Each subsystem is a module (its own crate) that
//! plugs into an [`App`](core::App):
//!
//! | Module | Crate | Role |
//! |---|---|---|
//! | ecs | `kit_ecs` | entities, components, resources |
//! | core | `kit_core` | `App`, `Module`, stages, time, console |
//! | scene | `kit_scene` | transforms and hierarchy |
//! | input | `kit_input` | keyboard and mouse |
//! | resource | `kit_resource` | project files and assets |
//! | gamesys | `kit_gamesys` | camera, model, sprite, light, script components; `.kscene` files |
//! | graphics | `kit_graphics` | Vulkan device, swapchain, pipelines |
//! | render | `kit_render` | draws scenes |
//! | x | `kit_x` | the x language |
//! | script | `kit_script` | runs x scripts on entities |
//! | collider | `kit_collider` | collision shapes, contact/trigger events, ray casts |
//! | physics | `kit_physics` | rigid-body dynamics |
//! | path | `kit_path` | follows paths sketched in Canvas mode |
//! | audio | `kit_audio` | plays sounds, with proximity (3D) fading and panning |
//! | engine | `kit_engine` | the windowed runtime |

pub use glam as math;
pub use kit_collider as collider;
pub use kit_diagram as diagram;
pub use kit_gamedata as gamedata;
pub use kit_core as core;
pub use kit_ecs as ecs;
pub use kit_engine as engine;
pub use kit_gamesys as gamesys;
pub use kit_graphics as graphics;
pub use kit_input as input;
pub use kit_path as path;
pub use kit_audio as audio;
pub use kit_physics as physics;
pub use kit_render as render;
pub use kit_resource as resource;
pub use kit_scene as scene;
pub use kit_script as script;
pub use kit_x as x;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub use kit_core::{BUILD_ID, BUILD_MARKER};

/// Common imports for games written in Rust against Kit.
pub mod prelude {
    pub use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
    pub use kit_core::{App, Console, Module, RunMode, Stage, Time};
    pub use kit_ecs::{Entity, World};
    pub use kit_engine::{EngineConfig, run};
    pub use kit_gamesys::{
        BodyKind, Camera, Collider, ColliderShape, Light, MeshKind, Model, PathFollow, PathLoop, Physics, Projection,
        RigidBody, Script, Sprite,
    };
    pub use kit_input::Input;
    pub use kit_scene::{GlobalTransform, Name, Transform, set_parent, spawn_node};
}
