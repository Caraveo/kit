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
mod particles;
mod fog;
mod physics;
pub mod post;
pub mod rig;
mod scene_file;
pub mod scenes;
mod terrain;

pub use particles::ParticleEmitter;
pub use fog::{FogEmitter, FogKind};
pub use terrain::{RESOLUTIONS as TERRAIN_RESOLUTIONS, SculptTool, Terrain};

pub use physics::{
    BodyKind, Collider, ColliderShape, CollisionEvent, CollisionEvents, PathFollow, PathLoop, Physics, PhysicsApi,
    RayHit, RigidBody,
};

pub use audio::{
    AudioCommand, AudioCommands, AudioSource, AudioState, Rolloff, SOUND_EXTENSIONS, is_sound, stereo_pan,
};
pub use bridge::{BridgeValue, Diagram, GraphEvent, GraphEvents, ScriptCall, ScriptCalls};
pub use post::{
    AmbientFx, AoQuality, ColorFx, ExposureFx, FocusFx, FogFx, GlowFx, ImageFx, Override, PostEffects, PostVolume,
    Tonemap, VolumeAffect, VolumeShape,
};
pub use rig::{Rig, RigClip, RigKey, RigPose, RigShape, RigTrack, RigVolume};
pub use scene_file::{
    EntityData, ExtraData, SCENE_EXTENSION, SceneFile, clone_subtree, load_scene, save_scene, save_subtree,
};

/// File extension of a Fab: a saved object to place or spawn from anywhere.
pub const FAB_EXTENSION: &str = "kfab";

use glam::{Mat4, Vec2, Vec3, Vec4};
use kit_core::{App, Module};
use serde::{Deserialize, Serialize};

/// A reusable `.xui` asset attached to a camera. Runtime UI state lives in
/// `kit_xui` and is keyed by the owning camera.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceAttachment {
    pub asset: String,
}

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
    /// Seconds into the animation (already scaled by speed), once a script
    /// starts one with `Model.Play`; until then animations follow game time.
    #[serde(skip)]
    pub clock: Option<f64>,
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
            clock: None,
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
        app.world.insert_resource(DebugDraw::default());
        app.world.insert_resource(InterfaceEdits::default());
        app.world.insert_resource(CinematicControl::default());
        app.world.insert_resource(CinematicOverlay::default());
        app.add_play_system(kit_core::Stage::First, "clear_scene_events", scenes::clear_scene_events);
        app.add_play_system(kit_core::Stage::First, "debug_draw_expire", expire_debug_draw);
        app.add_play_system(kit_core::Stage::Update, "animation_clocks", advance_animation_clocks);
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

/// Features (components) a script switched off with `feature.Enabled: FALSE`
/// (those without a switch of their own, unlike Model and Sprite visibility).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisabledFeatures(pub Vec<String>);

/// Has a script switched this feature off on `e`?
pub fn feature_disabled(world: &kit_ecs::World, e: kit_ecs::Entity, feature: &str) -> bool {
    world.get::<DisabledFeatures>(e).is_some_and(|d| d.0.iter().any(|f| f.eq_ignore_ascii_case(feature)))
}

/// Labels for finding groups of objects (`Scene.FindTagged("Enemy")`), saved
/// with the scene.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tags(pub Vec<String>);

/// Advance the animation clocks scripts started (`Model.Play`), by speed.
pub fn advance_animation_clocks(world: &mut kit_ecs::World) {
    let dt = world.resource::<kit_core::Time>().delta as f64;
    if world.storage::<Model>().is_none() {
        return;
    }
    let mut models = world.storage_mut::<Model>();
    for (_, m) in models.iter_mut() {
        if let Some(c) = m.clock.as_mut() {
            if m.animation_enabled {
                *c += dt * m.animation_speed.max(0.0) as f64;
            }
        }
    }
}

/// A line drawn by `Debug.Line` and friends (shown in the editor's viewport).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebugLine {
    pub from: glam::Vec3,
    pub to: glam::Vec3,
    pub color: glam::Vec4,
    /// Game time after which it disappears (this frame only by default).
    pub until: f64,
}

/// Lines scripts asked to see this frame.
#[derive(Default, Debug)]
pub struct DebugDraw {
    pub lines: Vec<DebugLine>,
}

fn expire_debug_draw(world: &mut kit_ecs::World) {
    let now = world.resource::<kit_core::Time>().elapsed;
    if let Some(mut d) = world.try_resource_mut::<DebugDraw>() {
        d.lines.retain(|l| l.until >= now);
    }
}

/// Changes scripts make to the on-screen interface (`Interface.SetText`,
/// `Show`, `Hide`, `SetColor`), applied by the interface module by element id.
#[derive(Default, Debug, Clone)]
pub struct InterfaceEdits {
    /// Element id → attribute → value (`text`, `color`, `background`…).
    pub attributes: std::collections::HashMap<String, Vec<(String, String)>>,
    /// Element id → shown or hidden.
    pub visible: std::collections::HashMap<String, bool>,
}

/// What scripts ask of the Cinematics Circuit (`Cinematic.Play("Intro")`).
#[derive(Clone, Debug, PartialEq)]
pub enum CinematicRequest {
    Play(String),
    Stop,
    /// Jump to the end (like the player pressing the skip key).
    Skip,
}

/// The bridge between scripts and the Cinematics Circuit (which lives in its
/// own crate, outside the script module).
#[derive(Default, Debug, Clone)]
pub struct CinematicControl {
    /// The project has the Cinematics Circuit installed and enabled.
    pub available: bool,
    pub requests: Vec<CinematicRequest>,
    /// The cinematic playing now, by name.
    pub playing: Option<String>,
    /// Which shot (0-based) and how far into the whole cinematic, in seconds.
    pub shot: usize,
    pub time: f32,
}

/// Drawn over the game while a cinematic plays: letterbox bars, a fade to
/// black and a caption (painted by the interface module, in every player).
#[derive(Default, Debug, Clone, PartialEq)]
pub struct CinematicOverlay {
    /// 0 = no bars, 1 = full letterbox.
    pub letterbox: f32,
    /// 0 = clear, 1 = black.
    pub fade: f32,
    pub caption: String,
}

impl CinematicOverlay {
    pub fn is_visible(&self) -> bool {
        self.letterbox > 0.001 || self.fade > 0.001 || !self.caption.is_empty()
    }
}
