//! What to draw, independent of the GPU API: the per-frame uniforms and a
//! draw list (models, imported meshes, terrain, sprites) in the order the
//! renderer must issue them. The Vulkan renderer and the web player
//! (WebGPU) both consume this, so they draw the same thing.

use crate::meshes::MeshData;
use crate::model_asset::{self, Material, ModelAsset};
use crate::{GridPlane, View, is_model_bundle, srgb_to_linear};
use anyhow::{Context, Result};
use glam::{Mat4, Vec3, Vec4};
use kit_ecs::{Entity, World};
use kit_gamesys::{Light, MeshKind, Model, Sprite, Terrain};
use kit_resource::Resources;
use kit_scene::GlobalTransform;
use std::collections::HashMap;
use std::path::Path;

/// Per-frame shader uniforms (`Frame` in shaders.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FrameUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub light_dir: [f32; 4],
    pub light_color: [f32; 4],
    pub params: [f32; 4],
}

/// Per-draw shader data (`Push` in shaders.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawUniforms {
    pub model: [[f32; 4]; 4],
    pub color: [f32; 4],
    pub extra: [f32; 4],
    pub surface: [f32; 4],
    pub emission: [f32; 4],
}

/// The scene's key light: first Light entity, else a default.
pub fn key_light(world: &World) -> (Vec3, Vec3) {
    let mut dir = Vec3::new(-0.45, -0.8, -0.4).normalize();
    let mut color = Vec3::splat(1.05);
    if let Some(lights) = world.storage::<Light>() {
        if let Some((e, l)) = lights.iter().next() {
            if let Some(g) = world.get::<GlobalTransform>(e) {
                dir = g.0.transform_vector3(Vec3::NEG_Z).normalize_or(dir);
            }
            color = l.color * l.intensity;
        }
    }
    (dir, color)
}

pub fn frame_uniforms(world: &World, view: &View) -> FrameUniforms {
    let (light_dir, light_color) = key_light(world);
    FrameUniforms {
        view_proj: view.view_proj().to_cols_array_2d(),
        camera_pos: view.camera_pos.extend(1.0).into(),
        light_dir: light_dir.extend(0.0).into(),
        light_color: light_color.extend(1.0).into(),
        params: [
            view.lighting as u32 as f32,
            view.time,
            if view.grid == Some(GridPlane::XY) { 1.0 } else { 0.0 },
            view.grid_fade,
        ],
    }
}

/// A range of an imported mesh drawn with one material.
#[derive(Clone, Copy, Debug)]
pub struct MeshPart {
    pub material: usize,
    pub first: u32,
    pub count: u32,
}

/// An imported model (`.kmodel` bundle or `.obj`) loaded on the CPU.
pub struct ImportedGeometry {
    /// Vertices and indices; the GPU renderer may drop these after uploading.
    pub data: MeshData,
    pub asset: Option<ModelAsset>,
    /// Per animation frame: the parts to draw.
    pub frames: Vec<Vec<MeshPart>>,
}

impl ImportedGeometry {
    pub fn load(resources: &Resources, path: &str) -> Result<Self> {
        let mut data = MeshData::default();
        let mut frames = Vec::new();
        let asset = if is_model_bundle(Path::new(path)) {
            let asset = ModelAsset::load(resources, path)?;
            let mut geometry = HashMap::new();
            for frame in &asset.frames {
                let mut parts = Vec::new();
                for (material, file) in frame.iter().enumerate() {
                    let Some(file) = file else { continue };
                    let (first, count) = if let Some(&range) = geometry.get(file) {
                        range
                    } else {
                        let vertices = model_asset::read_frame_vertices(resources, path, file)?;
                        let first = u32::try_from(data.indices.len())?;
                        let count = u32::try_from(vertices.len())?;
                        let end = first.checked_add(count).context("imported mesh exceeds index limits")?;
                        data.vertices.extend(vertices);
                        data.indices.extend(first..end);
                        geometry.insert(file, (first, count));
                        (first, count)
                    };
                    parts.push(MeshPart { material, first, count });
                }
                frames.push(parts);
            }
            Some(asset)
        } else {
            data = crate::obj::parse_obj(&resources.read_text(path)?).with_context(|| format!("loading {path}"))?;
            frames.push(vec![MeshPart { material: 0, first: 0, count: u32::try_from(data.indices.len())? }]);
            None
        };
        anyhow::ensure!(!data.indices.is_empty(), "imported model has no geometry");
        Ok(Self { data, asset, frames })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Geometry {
    Builtin(MeshKind),
    /// An imported model, by its project path.
    Imported(String),
    /// A mesh deformed on the CPU for this entity (see [`Deformed`]).
    Deformed(Entity),
}

/// A mesh a module deformed on the CPU (e.g. a volume rig), drawn instead of
/// the model's imported mesh. Triangle soup: vertices 0..n are the corners.
#[derive(Clone, Debug, Default)]
pub struct DeformedMesh {
    /// Bumped whenever `vertices` (or `colors`) change, so renderers re-upload.
    pub revision: u64,
    pub vertices: Vec<crate::Vertex>,
    /// Per-material ranges, as in the imported model's frame.
    pub parts: Vec<MeshPart>,
    /// Per-vertex colors: when set, the mesh is drawn as a lit colored surface
    /// instead of with its materials (influence previews).
    pub colors: Option<Vec<[f32; 3]>>,
}

impl DeformedMesh {
    /// Vertices in the colored (terrain) layout, when `colors` is set.
    pub fn colored_vertices(&self) -> Option<Vec<crate::TerrainVertex>> {
        let colors = self.colors.as_ref()?;
        Some(
            self.vertices
                .iter()
                .zip(colors)
                .map(|(v, c)| crate::TerrainVertex { pos: v.pos, normal: v.normal, uv: v.uv, color: *c })
                .collect(),
        )
    }
}

/// Deformed meshes by entity (a world resource; modules fill it).
#[derive(Default)]
pub struct Deformed {
    pub meshes: std::collections::HashMap<Entity, DeformedMesh>,
}

/// An editor helper shape, drawn see-through over the scene.
#[derive(Clone, Debug)]
pub struct Overlay {
    pub mesh: MeshKind,
    pub transform: Mat4,
    /// sRGB color with alpha.
    pub color: Vec4,
}

/// Which pipeline a draw uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// Lit mesh, depth-written (wireframe when the view asks for it).
    Opaque,
    /// Lit mesh, alpha-blended, no depth write.
    Blended,
    /// Unlit sprite, alpha-blended.
    Sprite,
    /// Lit surface with per-vertex colors (terrain shader; influence previews).
    Colored,
}

#[derive(Clone, Debug)]
pub struct DrawItem {
    pub geometry: Geometry,
    /// Index range; `count == None` draws the whole mesh.
    pub first: u32,
    pub count: Option<u32>,
    pub pass: Pass,
    /// Texture path; empty = plain white.
    pub texture: String,
    pub uniforms: DrawUniforms,
}

#[derive(Clone, Debug)]
pub struct TerrainDraw {
    pub entity: Entity,
    pub uniforms: DrawUniforms,
}

/// Everything to draw this frame, in order: opaque models, terrain, then
/// (after the grid) blended models and sprites back to front.
#[derive(Default)]
pub struct DrawList {
    pub opaque: Vec<DrawItem>,
    pub terrains: Vec<TerrainDraw>,
    pub blended: Vec<DrawItem>,
}

fn uniforms(model: Mat4, color: Vec4, extra: [f32; 4], surface: [f32; 4], emission: [f32; 4]) -> DrawUniforms {
    DrawUniforms { model: model.to_cols_array_2d(), color: color.into(), extra, surface, emission }
}

/// Build the draw list. `imported` looks up a loaded imported model by its
/// project path (models that aren't loaded yet are skipped).
pub fn draw_list<'a>(world: &World, view: &View, imported: impl Fn(&str) -> Option<&'a ImportedGeometry>) -> DrawList {
    let mut list = DrawList::default();
    let mut blended: Vec<(f32, DrawItem)> = Vec::new();
    let elapsed = world.try_resource::<kit_core::Time>().map_or(0.0, |time| time.elapsed);
    let deformed = world.try_resource::<Deformed>();
    let deformed = deformed.as_deref();
    if let Some(models) = world.storage::<Model>() {
        for (e, model) in models.iter().filter(|(_, model)| model.visible) {
            let Some(transform) = world.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
            let selected = view.selected.contains(&e) as u32 as f32;
            let ghost = view.ghosts.contains(&e);
            let mut part = |geometry: Geometry, first, count, material: Option<&Material>| {
                let mut color = srgb_to_linear(model.color);
                let mut texture = model.texture.clone();
                let mut unlit = model.unlit;
                if ghost {
                    color.w *= 0.32;
                }
                let mut blend = color.w < 0.999;
                let mut cutoff = 0.0;
                let mut double_sided = view.wireframe;
                if let Some(material) = material {
                    color *= Vec4::from_array(material.color);
                    if model.texture.is_empty() {
                        if let Some(name) = &material.texture {
                            texture = model_asset::sibling(&model.mesh_asset, name);
                        }
                    }
                    unlit |= material.unlit;
                    blend |= material.blended;
                    cutoff = material.cutoff;
                    double_sided |= material.double_sided;
                }
                let item = DrawItem {
                    geometry,
                    first,
                    count,
                    pass: Pass::Opaque,
                    texture,
                    uniforms: uniforms(
                        transform,
                        color,
                        [-(double_sided as u32 as f32), -cutoff, selected, unlit as u32 as f32],
                        material.map_or([0.0; 4], |m| [m.roughness, m.metallic, 1.0, 0.0]),
                        material.map_or([0.0; 4], |m| [m.emission[0], m.emission[1], m.emission[2], 0.0]),
                    ),
                };
                if blend && !view.wireframe {
                    blended.push(((view.view * transform.w_axis).z, DrawItem { pass: Pass::Blended, ..item }));
                } else {
                    list.opaque.push(item);
                }
            };
            if let Some(mesh) = deformed.and_then(|d| d.meshes.get(&e)) {
                if mesh.colors.is_some() {
                    list.opaque.push(DrawItem {
                        geometry: Geometry::Deformed(e),
                        first: 0,
                        count: None,
                        pass: Pass::Colored,
                        texture: String::new(),
                        uniforms: uniforms(transform, Vec4::ONE, [0.0, 0.0, selected, 0.0], [0.0; 4], [0.0; 4]),
                    });
                } else {
                    let asset = imported(&model.mesh_asset).and_then(|g| g.asset.as_ref());
                    for range in &mesh.parts {
                        let material = asset.and_then(|a| a.materials.get(range.material));
                        part(Geometry::Deformed(e), range.first, Some(range.count), material);
                    }
                }
            } else if model.mesh_asset.is_empty() {
                part(Geometry::Builtin(model.mesh), 0, None, None);
            } else if let Some(geometry) = imported(&model.mesh_asset) {
                let frame = geometry.asset.as_ref().filter(|_| model.animation_enabled).map_or(0, |asset| {
                    asset.frame(&model.animation, elapsed, model.animation_speed, model.animation_loop)
                });
                for range in &geometry.frames[frame] {
                    let material = geometry.asset.as_ref().map(|asset| &asset.materials[range.material]);
                    part(Geometry::Imported(model.mesh_asset.clone()), range.first, Some(range.count), material);
                }
            }
        }
    }
    if let Some(terrains) = world.storage::<Terrain>() {
        for (e, _) in terrains.iter() {
            let Some(g) = world.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
            let sel = view.selected.contains(&e) as u32 as f32;
            list.terrains.push(TerrainDraw {
                entity: e,
                uniforms: uniforms(g, Vec4::ONE, [0.0, 0.0, sel, 0.0], [0.0; 4], [0.0; 4]),
            });
        }
    }
    if let Some(sprites) = world.storage::<Sprite>() {
        for (e, sprite) in sprites.iter().filter(|(_, sprite)| sprite.visible) {
            let Some(g) = world.get::<GlobalTransform>(e) else { continue };
            let selected = view.selected.contains(&e) as u32 as f32;
            let item = DrawItem {
                geometry: Geometry::Builtin(MeshKind::Quad),
                first: 0,
                count: None,
                pass: Pass::Sprite,
                texture: sprite.texture.clone(),
                uniforms: uniforms(
                    g.0 * Mat4::from_scale(sprite.size.extend(1.0)),
                    srgb_to_linear(sprite.color),
                    [sprite.flip_x as u32 as f32, sprite.flip_y as u32 as f32, selected, 1.0],
                    [0.0; 4],
                    [0.0; 4],
                ),
            };
            blended.push(((view.view * g.0.w_axis).z, item));
        }
    }
    blended.sort_by(|a, b| a.0.total_cmp(&b.0));
    list.blended = blended.into_iter().map(|(_, item)| item).collect();
    // Editor helpers last, so they show through the (ghosted) scene.
    for o in &view.overlay {
        list.blended.push(DrawItem {
            geometry: Geometry::Builtin(o.mesh),
            first: 0,
            count: None,
            pass: Pass::Blended,
            texture: String::new(),
            uniforms: uniforms(o.transform, srgb_to_linear(o.color), [-1.0, 0.0, 0.0, 1.0], [0.0; 4], [0.0; 4]),
        });
    }
    list
}

/// The grid plane's transform for a view (drawn with the built-in Plane mesh).
pub fn grid_transform(view: &View) -> Option<Mat4> {
    let size = view.grid_fade * 2.5;
    Some(match view.grid? {
        GridPlane::XZ => {
            Mat4::from_translation(Vec3::new(view.camera_pos.x.round(), 0.0, view.camera_pos.z.round()))
                * Mat4::from_scale(Vec3::new(size, 1.0, size))
        }
        GridPlane::XY => {
            Mat4::from_translation(Vec3::new(view.camera_pos.x.round(), view.camera_pos.y.round(), -0.001))
                * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
                * Mat4::from_scale(Vec3::new(size, 1.0, size))
        }
    })
}

/// Project paths of imported models the world uses.
pub fn wanted_models(world: &World) -> Vec<String> {
    let mut out: Vec<String> = world
        .storage::<Model>()
        .map(|models| models.iter().map(|(_, m)| m.mesh_asset.clone()).filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

/// Textures the world uses (model, material and sprite textures).
pub fn wanted_textures<'a>(world: &World, imported: impl Fn(&str) -> Option<&'a ImportedGeometry>) -> Vec<String> {
    let mut wanted: Vec<String> = Vec::new();
    if let Some(models) = world.storage::<Model>() {
        for (_, model) in models.iter() {
            if !model.texture.is_empty() {
                wanted.push(model.texture.clone());
            }
            if let Some(asset) = imported(&model.mesh_asset).and_then(|g| g.asset.as_ref()) {
                wanted.extend(
                    asset
                        .materials
                        .iter()
                        .filter_map(|m| m.texture.as_ref())
                        .map(|texture| model_asset::sibling(&model.mesh_asset, texture)),
                );
            }
        }
    }
    if let Some(sprites) = world.storage::<Sprite>() {
        wanted.extend(sprites.iter().map(|(_, s)| s.texture.clone()));
    }
    wanted.retain(|t| !t.trim().is_empty());
    wanted.sort();
    wanted.dedup();
    wanted
}
