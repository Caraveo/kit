//! # kit_gamesys
//!
//! The component types games are built from (Defold's `gamesys`: sprite, model,
//! camera, light, script) and the `.kscene` file format (Defold's collections).
//!
//! 2D and 3D are peers: a [`Sprite`] is a textured quad in world space and a
//! [`Model`] is a mesh. An orthographic [`Camera`] gives you a 2D game, a
//! perspective one a 3D game, and both kinds of objects can share a scene.

pub mod audio;
pub mod bridge;
mod physics;
pub mod post;
pub mod rig;
mod scene_file;
pub mod scenes;
mod terrain;

pub use terrain::{RESOLUTIONS as TERRAIN_RESOLUTIONS, SculptTool, Terrain};

pub use physics::{
    BodyKind, Collider, ColliderShape, CollisionEvent, CollisionEvents, PathFollow, PathLoop, Physics, PhysicsApi,
    RayHit, RigidBody,
};

pub use audio::{
    AudioCommand, AudioCommands, AudioSource, AudioState, Rolloff, SOUND_EXTENSIONS, is_sound, stereo_pan,
};
pub use bridge::{BridgeValue, Diagram, GraphEvent, GraphEvents, ScriptCall, ScriptCalls};
pub use rig::{Rig, RigClip, RigKey, RigPose, RigShape, RigTrack, RigVolume};
pub use post::{
    AmbientFx, AoQuality, ColorFx, ExposureFx, FocusFx, GlowFx, ImageFx, Override, PostEffects, PostVolume, Tonemap,
    VolumeAffect, VolumeShape,
};
pub use scene_file::{EntityData, ExtraData, SCENE_EXTENSION, SceneFile, clone_subtree, load_scene, save_scene};

use glam::{Mat4, Vec2, Vec3, Vec4};
use kit_core::{App, Module};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Projection {
    Perspective {
        /// Vertical field of view in degrees.
        fov_y: f32,
        near: f32,
        far: f32,
    },
    Orthographic {
        /// Visible world units from bottom to top of the screen.
        height: f32,
        near: f32,
        far: f32,
    },
}

impl Projection {
    /// Vulkan-style projection (Y down in clip space, depth 0..1).
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(1e-4);
        match *self {
            Projection::Perspective { fov_y, near, far } => glam::camera::rh::proj::vulkan::perspective(
                fov_y.to_radians().clamp(0.01, 3.1),
                aspect,
                near.max(1e-4),
                far.max(near + 1e-3),
            ),
            Projection::Orthographic { height, near, far } => {
                let h = height.max(1e-4) * 0.5;
                let w = h * aspect;
                glam::camera::rh::proj::vulkan::orthographic(-w, w, -h, h, near, far.max(near + 1e-3))
            }
        }
    }

    pub fn is_orthographic(&self) -> bool {
        matches!(self, Projection::Orthographic { .. })
    }
}

/// A camera. The renderer uses the first active camera in hierarchy order.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Camera {
    pub projection: Projection,
    pub clear_color: Vec4,
    pub active: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self::perspective()
    }
}

impl Camera {
    pub fn perspective() -> Self {
        Self {
            projection: Projection::Perspective { fov_y: 50.0, near: 0.05, far: 500.0 },
            clear_color: Vec4::new(0.063, 0.063, 0.09, 1.0),
            active: true,
        }
    }
    pub fn orthographic(height: f32) -> Self {
        Self { projection: Projection::Orthographic { height, near: -100.0, far: 100.0 }, ..Self::perspective() }
    }
}

/// Built-in meshes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MeshKind {
    #[default]
    Cube,
    Sphere,
    Plane,
    Quad,
    Cylinder,
    Torus,
}

impl MeshKind {
    pub const ALL: [MeshKind; 6] =
        [MeshKind::Cube, MeshKind::Sphere, MeshKind::Plane, MeshKind::Quad, MeshKind::Cylinder, MeshKind::Torus];

    pub fn label(self) -> &'static str {
        match self {
            MeshKind::Cube => "Cube",
            MeshKind::Sphere => "Sphere",
            MeshKind::Plane => "Plane",
            MeshKind::Quad => "Quad",
            MeshKind::Cylinder => "Cylinder",
            MeshKind::Torus => "Torus",
        }
    }

    /// Local-space bounding radius (meshes are unit sized around the origin).
    pub fn bounding_radius(self) -> f32 {
        match self {
            MeshKind::Cube => 0.87,
            MeshKind::Sphere | MeshKind::Cylinder => 0.71,
            MeshKind::Plane | MeshKind::Quad => 0.71,
            MeshKind::Torus => 0.65,
        }
    }
}

/// A 3D mesh with a material color and optional texture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Model {
    pub mesh: MeshKind,
    /// Project-relative imported mesh path; empty for a built-in mesh.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mesh_asset: String,
    /// Empty selects the asset's first animation.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub animation: String,
    pub animation_speed: f32,
    pub animation_loop: bool,
    pub animation_enabled: bool,
    pub color: Vec4,
    /// Project-relative image path; empty for none.
    pub texture: String,
    pub unlit: bool,
    pub visible: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            mesh: MeshKind::Cube,
            mesh_asset: String::new(),
            animation: String::new(),
            animation_speed: 1.0,
            animation_loop: true,
            animation_enabled: true,
            color: Vec4::new(0.76, 0.66, 1.0, 1.0),
            texture: String::new(),
            unlit: false,
            visible: true,
        }
    }
}

/// A 2D sprite: a textured, alpha-blended quad in world space (XY plane).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sprite {
    /// Size in world units.
    pub size: Vec2,
    pub color: Vec4,
    /// Project-relative image path; empty for a solid color.
    pub texture: String,
    pub flip_x: bool,
    pub flip_y: bool,
    pub visible: bool,
}

impl Default for Sprite {
    fn default() -> Self {
        Self { size: Vec2::ONE, color: Vec4::ONE, texture: String::new(), flip_x: false, flip_y: false, visible: true }
    }
}

/// A directional light shining along the entity's forward (-Z) axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Light {
    pub color: Vec3,
    pub intensity: f32,
}

impl Default for Light {
    fn default() -> Self {
        Self { color: Vec3::new(1.0, 0.97, 0.94), intensity: 1.0 }
    }
}

/// An x script attached to an entity.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Script {
    /// Project-relative path to the `.x` file.
    pub path: String,
    /// Values set in the editor for the script's EDIT fields (field name →
    /// value). Applied when the script starts, before `Init`.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub values: std::collections::BTreeMap<String, FieldValue>,
}

impl Script {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), values: Default::default() }
    }
}

/// A value for a script's EDIT field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Vector([f64; 3]),
    Text(String),
}

/// Registers the game-system module.
pub struct GameSysModule;

impl Module for GameSysModule {
    fn name(&self) -> &'static str {
        "gamesys"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["scene"]
    }
    fn build(&self, app: &mut App) {
        app.world.insert_resource(AudioCommands::default());
        app.world.insert_resource(AudioState::default());
        app.world.insert_resource(GraphEvents::default());
        app.world.insert_resource(ScriptCalls::default());
        app.world.insert_resource(scenes::SceneRequests::default());
        app.world.insert_resource(scenes::SceneEvents::default());
        app.add_play_system(kit_core::Stage::First, "clear_scene_events", scenes::clear_scene_events);
        app.add_play_system(kit_core::Stage::Last, "scene_requests", scenes::apply_scene_requests);
    }
}

/// The active camera entity: the first active camera in hierarchy order.
pub fn active_camera(world: &kit_ecs::World) -> Option<kit_ecs::Entity> {
    let cams = world.storage::<Camera>()?;
    let mut order = Vec::new();
    for root in kit_scene::roots(world) {
        order.extend(kit_scene::descendants_and_self(world, root));
    }
    order.into_iter().find(|e| cams.get(*e).is_some_and(|c| c.active))
}
