//! # kit_render
//!
//! Draws a Kit world (Defold's `render` module): 3D [`Model`]s, 2D [`Sprite`]s,
//! terrain, the editor grid and Dimension's gradient backdrop.
//!
//! [`draw`] decides *what* to draw each frame, independent of the GPU API. The
//! Vulkan [`Renderer`] (feature `vulkan`, on by default) draws it on desktop;
//! the web player draws the same list with WebGPU.
//!
//! ```ignore
//! renderer.prepare(&gfx, &world)?;                  // upload textures (outside a frame)
//! let view = View::from_camera(&world, aspect)?;    // or an editor camera
//! renderer.render(&gfx, cmd, frame_index, &target, &view, &world)?;
//! ```
//!
//! Colors in components are sRGB (what you pick in the editor); the renderer
//! converts them to linear space.
//!
//! [`Model`]: kit_gamesys::Model
//! [`Sprite`]: kit_gamesys::Sprite

pub mod draw;
mod meshes;
pub mod model_asset;
mod obj;
pub mod pick;
#[cfg(feature = "vulkan")]
mod vulkan;

pub use meshes::{MeshData, Vertex, build as build_mesh};
pub use obj::{load_obj, parse_obj};
#[cfg(feature = "vulkan")]
pub use vulkan::{Renderer, Target};

/// The built-in shaders (WGSL), shared by the Vulkan renderer and the web player.
pub const SHADER_SRC: &str = include_str!("shaders.wgsl");

use glam::{Mat4, Vec3, Vec4};
use kit_ecs::{Entity, World};
use kit_gamesys::{Camera, Terrain};
use kit_resource::Resources;
use kit_scene::GlobalTransform;
use model_asset::ModelAsset;
use std::path::Path;

/// Local bounds of an imported model, for editor framing and picking.
pub fn model_bounds(world: &World, asset: &str) -> Option<(Vec3, Vec3)> {
    if asset.is_empty() {
        return None;
    }
    let resources = world.try_resource::<Resources>()?;
    if is_model_bundle(Path::new(asset)) {
        return ModelAsset::load(&resources, asset).ok().map(|data| data.bounds());
    }
    let mesh = load_obj(&resources.resolve(asset).ok()?).ok()?;
    Some(mesh.vertices.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), vertex| {
        let position = Vec3::from_array(vertex.pos);
        (lo.min(position), hi.max(position))
    }))
}

pub fn is_model_bundle(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("kmodel"))
}

/// Which plane the editor grid lies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridPlane {
    /// Ground plane, for 3D.
    XZ,
    /// Screen plane, for 2D.
    XY,
}

/// Everything about *how* to draw a frame: camera matrices and view options.
#[derive(Clone, Debug)]
pub struct View {
    pub view: Mat4,
    pub proj: Mat4,
    pub camera_pos: Vec3,
    /// sRGB clear color (ignored when `backdrop` is on).
    pub clear: Vec4,
    /// Draw Dimension's radial-gradient stage behind the scene.
    pub backdrop: bool,
    pub grid: Option<GridPlane>,
    /// Distance at which the grid fades out.
    pub grid_fade: f32,
    pub lighting: bool,
    pub wireframe: bool,
    /// Entities drawn with the selection highlight.
    pub selected: Vec<Entity>,
    pub time: f32,
    /// Apply post-processing volumes (game cameras; the editor's main view
    /// only when its "Volumes" preview is on).
    pub post: bool,
}

impl View {
    /// Build a view from the world's active [`Camera`].
    pub fn from_camera(world: &World, aspect: f32) -> Option<View> {
        Self::from_camera_entity(world, kit_gamesys::active_camera(world)?, aspect)
    }

    /// A view through a specific camera entity (the editor's camera preview).
    pub fn from_camera_entity(world: &World, e: Entity, aspect: f32) -> Option<View> {
        let cam = world.get::<Camera>(e)?;
        let g = world.get::<GlobalTransform>(e).map(|g| g.0).unwrap_or(Mat4::IDENTITY);
        let (_, rot, pos) = g.to_scale_rotation_translation();
        // Ignore scale on the camera.
        let world_m = Mat4::from_rotation_translation(rot, pos);
        Some(View {
            view: world_m.inverse(),
            proj: cam.projection.matrix(aspect),
            camera_pos: pos,
            clear: cam.clear_color,
            backdrop: false,
            grid: None,
            grid_fade: 80.0,
            lighting: true,
            wireframe: false,
            selected: Vec::new(),
            time: 0.0,
            post: true,
        })
    }

    /// A fallback view when the scene has no camera.
    pub fn fallback(aspect: f32) -> View {
        let eye = Vec3::new(0.0, 2.0, 8.0);
        View {
            view: glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y),
            proj: glam::camera::rh::proj::vulkan::perspective(50f32.to_radians(), aspect.max(1e-3), 0.05, 500.0),
            camera_pos: eye,
            clear: Vec4::new(0.063, 0.063, 0.09, 1.0),
            backdrop: false,
            grid: None,
            grid_fade: 80.0,
            lighting: true,
            wireframe: false,
            selected: Vec::new(),
            time: 0.0,
            post: true,
        }
    }

    pub fn view_proj(&self) -> Mat4 {
        self.proj * self.view
    }
}

/// The camera a view is drawn from, for post-processing. `ndc_y_up`: the
/// clip matrix has been flipped for WebGPU.
pub fn camera_info(view: &View, ndc_y_up: bool) -> kit_post::CameraInfo {
    let view_proj =
        if ndc_y_up { Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)) * view.view_proj() } else { view.view_proj() };
    kit_post::CameraInfo {
        view_proj,
        position: view.camera_pos,
        forward: view.view.inverse().transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z),
        // Orthographic projections keep w = 1.
        orthographic: view.proj.w_axis.w == 1.0 && view.proj.z_axis.w == 0.0,
        ndc_y_up,
    }
}

/// Converts an sRGB color (alpha untouched) to linear.
pub fn srgb_to_linear(c: Vec4) -> Vec4 {
    let f = |x: f32| if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) };
    Vec4::new(f(c.x), f(c.y), f(c.z), c.w)
}

/// Vertex of a terrain mesh: position, normal, uv, linear color.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerrainVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 3],
}

/// Triangulate a terrain grid (counter-clockwise when seen from above).
pub fn terrain_mesh(t: &Terrain) -> (Vec<TerrainVertex>, Vec<u32>) {
    let r = t.resolution;
    let mut verts = Vec::with_capacity((r * r) as usize);
    for z in 0..r {
        for x in 0..r {
            let p = t.vertex(x, z);
            let n = t.normal(x, z);
            let c = srgb_to_linear(t.color_for(p.y, n.y));
            verts.push(TerrainVertex {
                pos: p.into(),
                normal: n.into(),
                uv: [x as f32 / (r - 1) as f32, z as f32 / (r - 1) as f32],
                color: [c.x, c.y, c.z],
            });
        }
    }
    let mut idx = Vec::with_capacity(((r - 1) * (r - 1) * 6) as usize);
    for z in 0..r - 1 {
        for x in 0..r - 1 {
            let a = z * r + x;
            let b = a + 1;
            let c = a + r;
            let d = c + 1;
            idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    (verts, idx)
}

#[cfg(test)]
mod terrain_tests {
    use super::*;

    #[test]
    fn terrain_triangles_face_up() {
        let mut t = Terrain::flat(10.0, 5);
        t.generate(3, 2.0, 0.5);
        let (v, i) = terrain_mesh(&t);
        assert_eq!(v.len(), 25);
        for tri in i.chunks(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|k| Vec3::from(v[k as usize].pos));
            assert!((b - a).cross(c - a).y > 0.0, "counter-clockwise from above");
        }
    }
}
