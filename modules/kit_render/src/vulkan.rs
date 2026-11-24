//! The Vulkan renderer (desktop): draws the [`draw`](crate::draw) list with kit_graphics.

use crate::draw::{self, DrawUniforms, Geometry, ImportedGeometry, Pass};
use crate::meshes;
use crate::{SHADER_SRC, TerrainVertex, Vertex, View, srgb_to_linear, terrain_mesh};
use anyhow::Result;
use kit_ecs::{Entity, World};
use kit_gamesys::{MeshKind, Terrain};
use kit_graphics::{
    Blend, Buffer, DEPTH_FORMAT, Gfx, Image, MemoryLocation, PipelineDesc, ShaderModule, VertexLayout, vk,
};
use kit_resource::Resources;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

const MAX_VIEWS_PER_FRAME: usize = 4;
const FRAMES: usize = kit_graphics::FRAMES_IN_FLIGHT;
const UBO_STRIDE: u64 = 256;
const MAX_TEXTURES: u32 = 512;

/// The image the renderer draws into. It must be in `COLOR_ATTACHMENT_OPTIMAL`
/// and stays in that layout.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub image: vk::Image,
    pub view: vk::ImageView,
    pub extent: vk::Extent2D,
    pub format: vk::Format,
}

struct GpuMesh {
    vertices: Buffer,
    indices: Buffer,
    count: u32,
}

struct ImportedMesh {
    mesh: GpuMesh,
    geometry: ImportedGeometry,
}

impl ImportedMesh {
    fn load(gfx: &Gfx, resources: &Resources, path: &str) -> Result<Self> {
        let mut geometry = ImportedGeometry::load(resources, path)?;
        let data = std::mem::take(&mut geometry.data);
        let mut vertices =
            Buffer::with_data(gfx, vk::BufferUsageFlags::VERTEX_BUFFER, &data.vertices, "imported mesh")?;
        let indices = match Buffer::with_data(gfx, vk::BufferUsageFlags::INDEX_BUFFER, &data.indices, "imported mesh") {
            Ok(indices) => indices,
            Err(error) => {
                vertices.destroy(gfx);
                return Err(error);
            }
        };
        let mesh = GpuMesh { vertices, indices, count: data.indices.len() as u32 };
        Ok(Self { mesh, geometry })
    }
}

struct Texture {
    image: Image,
    set: vk::DescriptorSet,
    modified: Option<SystemTime>,
}

struct Pipelines {
    terrain: vk::Pipeline,
    terrain_wire: Option<vk::Pipeline>,
    mesh: vk::Pipeline,
    mesh_blend: vk::Pipeline,
    wire: Option<vk::Pipeline>,
    sprite: vk::Pipeline,
    grid: vk::Pipeline,
    backdrop: vk::Pipeline,
}

struct Attachments {
    format: vk::Format,
    extent: vk::Extent2D,
    color: Option<Image>,
    depth: Image,
}


pub struct Renderer {
    shader: ShaderModule,
    set0_layout: vk::DescriptorSetLayout,
    set1_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    ubo: Buffer,
    ubo_set: vk::DescriptorSet,
    pipelines: HashMap<vk::Format, Pipelines>,
    meshes: HashMap<MeshKind, GpuMesh>,
    imported_meshes: HashMap<PathBuf, Option<ImportedMesh>>,
    texture_root: Option<PathBuf>,
    /// Terrain meshes per entity, with the fingerprint they were built from.
    terrains: HashMap<Entity, (u64, GpuMesh)>,
    white: Texture,
    textures: HashMap<String, Texture>,
    failed_textures: HashMap<String, Option<SystemTime>>,
    /// MSAA/depth attachments per (format, size): the main view and previews.
    attachments: Vec<Attachments>,
    samples: vk::SampleCountFlags,
    last_frame: usize,
    view_slot: usize,
    last_reload_check: Instant,
}

impl Renderer {
    pub fn new(gfx: &Gfx) -> Result<Self> {
        let device = &gfx.device;
        let shader = kit_graphics::compile_wgsl(gfx, SHADER_SRC, "kit builtin")?;
        unsafe {
            let set0_layout = device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)]),
                None,
            )?;
            let set1_layout = device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(0)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(1)
                        .descriptor_type(vk::DescriptorType::SAMPLER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                ]),
                None,
            )?;
            let layouts = [set0_layout, set1_layout];
            let push = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .size(std::mem::size_of::<DrawUniforms>() as u32)];
            let layout = device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts).push_constant_ranges(&push),
                None,
            )?;
            let pool = device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                    .max_sets(MAX_TEXTURES + 1)
                    .pool_sizes(&[
                        vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, descriptor_count: 1 },
                        vk::DescriptorPoolSize {
                            ty: vk::DescriptorType::SAMPLED_IMAGE,
                            descriptor_count: MAX_TEXTURES,
                        },
                        vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: MAX_TEXTURES },
                    ]),
                None,
            )?;
            let sampler = device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::REPEAT)
                    .address_mode_v(vk::SamplerAddressMode::REPEAT)
                    .address_mode_w(vk::SamplerAddressMode::REPEAT)
                    .max_lod(vk::LOD_CLAMP_NONE),
                None,
            )?;

            let ubo_size = UBO_STRIDE * (FRAMES * MAX_VIEWS_PER_FRAME) as u64;
            let ubo = Buffer::new(
                gfx,
                ubo_size,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                MemoryLocation::CpuToGpu,
                "frame uniforms",
            )?;
            let ubo_set = device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default().descriptor_pool(pool).set_layouts(&[set0_layout]),
            )?[0];
            let info = [vk::DescriptorBufferInfo::default()
                .buffer(ubo.buffer)
                .offset(0)
                .range(std::mem::size_of::<draw::FrameUniforms>() as u64)];
            device.update_descriptor_sets(
                &[vk::WriteDescriptorSet::default()
                    .dst_set(ubo_set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                    .buffer_info(&info)],
                &[],
            );

            let mut meshes = HashMap::new();
            for kind in MeshKind::ALL {
                let data = meshes::build(kind);
                meshes.insert(
                    kind,
                    GpuMesh {
                        vertices: Buffer::with_data(
                            gfx,
                            vk::BufferUsageFlags::VERTEX_BUFFER,
                            &data.vertices,
                            kind.label(),
                        )?,
                        indices: Buffer::with_data(
                            gfx,
                            vk::BufferUsageFlags::INDEX_BUFFER,
                            &data.indices,
                            kind.label(),
                        )?,
                        count: data.indices.len() as u32,
                    },
                );
            }

            let mut r = Self {
                shader,
                set0_layout,
                set1_layout,
                layout,
                pool,
                sampler,
                ubo,
                ubo_set,
                pipelines: HashMap::new(),
                meshes,
                terrains: HashMap::new(),
                imported_meshes: HashMap::new(),
                texture_root: None,
                white: Texture {
                    image: Image::texture_rgba8(gfx, 1, 1, &[255; 4], true, "white")?,
                    set: vk::DescriptorSet::null(),
                    modified: None,
                },
                textures: HashMap::new(),
                failed_textures: HashMap::new(),
                attachments: Vec::new(),
                samples: gfx.caps.msaa,
                last_frame: usize::MAX,
                view_slot: 0,
                last_reload_check: Instant::now(),
            };
            r.white.set = r.texture_set(gfx, r.white.image.view)?;
            Ok(r)
        }
    }

    fn texture_set(&self, gfx: &Gfx, view: vk::ImageView) -> Result<vk::DescriptorSet> {
        unsafe {
            let set = gfx.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.pool).set_layouts(&[self.set1_layout]),
            )?[0];
            let img = [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let smp = [vk::DescriptorImageInfo::default().sampler(self.sampler)];
            gfx.device.update_descriptor_sets(
                &[
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(&img),
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::SAMPLER)
                        .image_info(&smp),
                ],
                &[],
            );
            Ok(set)
        }
    }

    fn pipelines_for(&mut self, gfx: &Gfx, format: vk::Format) -> Result<&Pipelines> {
        if !self.pipelines.contains_key(&format) {
            let vertex = VertexLayout {
                stride: std::mem::size_of::<Vertex>() as u32,
                attributes: vec![
                    (0, vk::Format::R32G32B32_SFLOAT, 0),
                    (1, vk::Format::R32G32B32_SFLOAT, 12),
                    (2, vk::Format::R32G32_SFLOAT, 24),
                ],
            };
            let base = |vs, fs| PipelineDesc {
                shader: &self.shader,
                vs_entry: vs,
                fs_entry: fs,
                layout: self.layout,
                vertex: Some(vertex.clone()),
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                cull: vk::CullModeFlags::BACK,
                polygon: vk::PolygonMode::FILL,
                depth_test: true,
                depth_write: true,
                blend: Blend::None,
                color_format: format,
                depth_format: Some(DEPTH_FORMAT),
                samples: self.samples,
            };
            let mesh = kit_graphics::create_pipeline(
                gfx,
                &PipelineDesc { cull: vk::CullModeFlags::NONE, ..base(c"vs_mesh", c"fs_mesh") },
            )?;
            let wire = if gfx.caps.wireframe {
                Some(kit_graphics::create_pipeline(
                    gfx,
                    &PipelineDesc {
                        polygon: vk::PolygonMode::LINE,
                        cull: vk::CullModeFlags::NONE,
                        ..base(c"vs_mesh", c"fs_mesh")
                    },
                )?)
            } else {
                None
            };
            let mesh_blend = kit_graphics::create_pipeline(
                gfx,
                &PipelineDesc {
                    cull: vk::CullModeFlags::NONE,
                    depth_write: false,
                    blend: Blend::Alpha,
                    ..base(c"vs_mesh", c"fs_mesh")
                },
            )?;
            let sprite = kit_graphics::create_pipeline(
                gfx,
                &PipelineDesc {
                    cull: vk::CullModeFlags::NONE,
                    depth_write: false,
                    blend: Blend::Alpha,
                    ..base(c"vs_mesh", c"fs_sprite")
                },
            )?;
            let grid = kit_graphics::create_pipeline(
                gfx,
                &PipelineDesc {
                    cull: vk::CullModeFlags::NONE,
                    depth_write: false,
                    blend: Blend::Alpha,
                    ..base(c"vs_mesh", c"fs_grid")
                },
            )?;
            let backdrop = kit_graphics::create_pipeline(
                gfx,
                &PipelineDesc {
                    vertex: None,
                    cull: vk::CullModeFlags::NONE,
                    depth_test: false,
                    depth_write: false,
                    ..base(c"vs_full", c"fs_backdrop")
                },
            )?;
            let terrain_vertex = VertexLayout {
                stride: std::mem::size_of::<TerrainVertex>() as u32,
                attributes: vec![
                    (0, vk::Format::R32G32B32_SFLOAT, 0),
                    (1, vk::Format::R32G32B32_SFLOAT, 12),
                    (2, vk::Format::R32G32_SFLOAT, 24),
                    (3, vk::Format::R32G32B32_SFLOAT, 32),
                ],
            };
            let terrain = kit_graphics::create_pipeline(
                gfx,
                &PipelineDesc { vertex: Some(terrain_vertex.clone()), ..base(c"vs_terrain", c"fs_terrain") },
            )?;
            let terrain_wire = if gfx.caps.wireframe {
                Some(kit_graphics::create_pipeline(
                    gfx,
                    &PipelineDesc {
                        vertex: Some(terrain_vertex),
                        polygon: vk::PolygonMode::LINE,
                        cull: vk::CullModeFlags::NONE,
                        ..base(c"vs_terrain", c"fs_terrain")
                    },
                )?)
            } else {
                None
            };
            self.pipelines
                .insert(format, Pipelines { terrain, terrain_wire, mesh, mesh_blend, wire, sprite, grid, backdrop });
        }
        Ok(&self.pipelines[&format])
    }

    fn ensure_attachments(&mut self, gfx: &Gfx, format: vk::Format, extent: vk::Extent2D) -> Result<usize> {
        if let Some(i) = self.attachments.iter().position(|a| a.format == format && a.extent == extent) {
            return Ok(i);
        }
        // Keep a few sizes (main view + previews); drop the oldest beyond that.
        if self.attachments.len() >= 3 {
            gfx.wait_idle();
            let mut old = self.attachments.remove(0);
            if let Some(mut c) = old.color.take() {
                c.destroy(gfx);
            }
            old.depth.destroy(gfx);
        }
        let color = if self.samples != vk::SampleCountFlags::TYPE_1 {
            Some(Image::new(
                gfx,
                extent,
                format,
                vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSIENT_ATTACHMENT,
                self.samples,
                "msaa color",
            )?)
        } else {
            None
        };
        let depth = Image::new(
            gfx,
            extent,
            DEPTH_FORMAT,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            self.samples,
            "depth",
        )?;
        self.attachments.push(Attachments { format, extent, color, depth });
        Ok(self.attachments.len() - 1)
    }

    /// Load (or hot-reload) textures referenced by the world. Call outside of
    /// command recording; uploads are synchronous.
    pub fn prepare(&mut self, gfx: &Gfx, world: &World) {
        self.prepare_terrains(gfx, world);
        let Some(res) = world.try_resource::<Resources>().map(|r| r.clone()) else { return };
        self.prepare_imported_meshes(gfx, world, &res);
        if self.texture_root.as_deref() != Some(res.root()) {
            let old = std::mem::take(&mut self.textures);
            for texture in old.into_values() {
                self.free_texture(gfx, texture);
            }
            self.failed_textures.clear();
            self.texture_root = Some(res.root().to_path_buf());
        }
        let wanted = draw::wanted_textures(world, |asset| self.imported(&res, asset));

        let check = self.last_reload_check.elapsed() > Duration::from_millis(700);
        if check {
            self.last_reload_check = Instant::now();
        }
        for path in wanted {
            let modified = if check || !self.textures.contains_key(&path) { res.modified(&path) } else { None };
            if let Some(t) = self.textures.get(&path) {
                if !check || modified == t.modified {
                    continue;
                }
            } else if let Some(m) = self.failed_textures.get(&path) {
                if !check || *m == modified {
                    continue;
                }
            }
            match res.load_image(&path) {
                Ok(img) => match Image::texture_rgba8(gfx, img.width, img.height, &img.rgba, true, &path)
                    .and_then(|image| Ok(Texture { set: self.texture_set(gfx, image.view)?, image, modified }))
                {
                    Ok(tex) => {
                        if let Some(old) = self.textures.insert(path.clone(), tex) {
                            self.free_texture(gfx, old);
                        }
                        self.failed_textures.remove(&path);
                        log::info!("texture loaded: {path}");
                    }
                    Err(e) => {
                        log::error!("texture {path}: {e:#}");
                        self.failed_textures.insert(path, modified);
                    }
                },
                Err(e) => {
                    if !self.failed_textures.contains_key(&path) {
                        if let Some(mut c) = world.try_resource_mut::<kit_core::Console>() {
                            c.error("render", format!("{e:#}"));
                        }
                    }
                    self.failed_textures.insert(path, modified);
                }
            }
        }
    }

    /// A loaded imported model by its project path.
    fn imported(&self, res: &Resources, asset: &str) -> Option<&ImportedGeometry> {
        let path = res.resolve(asset).ok()?;
        self.imported_meshes.get(&path)?.as_ref().map(|m| &m.geometry)
    }

    fn prepare_imported_meshes(&mut self, gfx: &Gfx, world: &World, resources: &Resources) {
        let wanted: HashMap<PathBuf, String> = draw::wanted_models(world)
            .into_iter()
            .filter_map(|asset| Some((resources.resolve(&asset).ok()?, asset)))
            .collect();
        let mut idle = false;
        self.imported_meshes.retain(|path, mesh| {
            if wanted.contains_key(path) {
                return true;
            }
            if let Some(imported) = mesh {
                if !idle {
                    gfx.wait_idle();
                    idle = true;
                }
                imported.mesh.vertices.destroy(gfx);
                imported.mesh.indices.destroy(gfx);
            }
            false
        });
        for (path, asset) in wanted {
            self.imported_meshes.entry(path.clone()).or_insert_with(|| {
                match ImportedMesh::load(gfx, resources, &asset) {
                    Ok(mesh) => Some(mesh),
                    Err(error) => {
                        if let Some(mut console) = world.try_resource_mut::<kit_core::Console>() {
                            console.error("render", format!("{}: {error:#}", path.display()));
                        }
                        None
                    }
                }
            });
        }
    }

    /// Build or rebuild terrain meshes whose heights or colors changed.
    fn prepare_terrains(&mut self, gfx: &Gfx, world: &World) {
        let live: Vec<(Entity, u64)> = world
            .storage::<Terrain>()
            .map(|s| s.iter().map(|(e, t)| (e, t.fingerprint())).collect())
            .unwrap_or_default();
        let stale: Vec<Entity> = self
            .terrains
            .iter()
            .filter(|(e, (fp, _))| !live.iter().any(|(le, lfp)| le == *e && lfp == fp))
            .map(|(e, _)| *e)
            .collect();
        if !stale.is_empty() {
            gfx.wait_idle();
            for e in stale {
                if let Some((_, mut m)) = self.terrains.remove(&e) {
                    m.vertices.destroy(gfx);
                    m.indices.destroy(gfx);
                }
            }
        }
        for (e, fp) in live {
            if self.terrains.contains_key(&e) {
                continue;
            }
            let t = world.get::<Terrain>(e).unwrap();
            let data = terrain_mesh(&t);
            let mesh = (|| -> Result<GpuMesh> {
                Ok(GpuMesh {
                    vertices: Buffer::with_data(gfx, vk::BufferUsageFlags::VERTEX_BUFFER, &data.0, "terrain")?,
                    indices: Buffer::with_data(gfx, vk::BufferUsageFlags::INDEX_BUFFER, &data.1, "terrain")?,
                    count: data.1.len() as u32,
                })
            })();
            match mesh {
                Ok(m) => {
                    self.terrains.insert(e, (fp, m));
                }
                Err(err) => log::error!("terrain mesh: {err:#}"),
            }
        }
    }

    fn free_texture(&mut self, gfx: &Gfx, mut t: Texture) {
        gfx.wait_idle();
        unsafe {
            let _ = gfx.device.free_descriptor_sets(self.pool, &[t.set]);
        }
        t.image.destroy(gfx);
    }

    fn texture(&self, path: &str) -> vk::DescriptorSet {
        if path.is_empty() {
            return self.white.set;
        }
        self.textures.get(path).map(|t| t.set).unwrap_or(self.white.set)
    }

    /// Record drawing commands for `world` as seen by `view` into `target`.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        gfx: &Gfx,
        cmd: vk::CommandBuffer,
        frame_index: usize,
        target: &Target,
        view: &View,
        world: &World,
    ) -> Result<()> {
        if target.extent.width == 0 || target.extent.height == 0 {
            return Ok(());
        }
        // Uniform slot: one per view per frame in flight.
        if frame_index != self.last_frame {
            self.last_frame = frame_index;
            self.view_slot = 0;
        }
        let slot = (frame_index % FRAMES) * MAX_VIEWS_PER_FRAME + self.view_slot.min(MAX_VIEWS_PER_FRAME - 1);
        self.view_slot += 1;

        let att_index = self.ensure_attachments(gfx, target.format, target.extent)?;
        self.pipelines_for(gfx, target.format)?;
        let device = &gfx.device;

        let uniforms = draw::frame_uniforms(world, view);
        let offset = slot as u64 * UBO_STRIDE;
        self.ubo.write(offset as usize, std::slice::from_ref(&uniforms));

        let att = &self.attachments[att_index];
        let p = &self.pipelines[&target.format];
        let clear = srgb_to_linear(view.clear);

        // Attachment layouts: contents are discarded each frame.
        if let Some(c) = &att.color {
            gfx.image_barrier(
                cmd,
                c.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
                (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
            );
        }
        gfx.image_barrier(
            cmd,
            att.depth.image,
            vk::ImageAspectFlags::DEPTH,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
            (vk::PipelineStageFlags::LATE_FRAGMENT_TESTS, vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE),
            (
                vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
            ),
        );

        let clear_value = vk::ClearValue { color: vk::ClearColorValue { float32: clear.to_array() } };
        let mut color_att = vk::RenderingAttachmentInfo::default()
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(clear_value);
        match &att.color {
            Some(msaa) => {
                color_att = color_att
                    .image_view(msaa.view)
                    .store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .resolve_mode(vk::ResolveModeFlags::AVERAGE)
                    .resolve_image_view(target.view)
                    .resolve_image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
            }
            None => color_att = color_att.image_view(target.view),
        }
        let depth_att = vk::RenderingAttachmentInfo::default()
            .image_view(att.depth.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::DONT_CARE)
            .clear_value(vk::ClearValue { depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 } });
        let colors = [color_att];
        let rendering = vk::RenderingInfo::default()
            .render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent: target.extent })
            .layer_count(1)
            .color_attachments(&colors)
            .depth_attachment(&depth_att);

        let push = |p: &DrawUniforms| {
            unsafe {
                device.cmd_push_constants(
                    cmd,
                    self.layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    bytemuck::bytes_of(p),
                );
            }
        };

        unsafe {
            device.cmd_begin_rendering(cmd, &rendering);
            device.cmd_set_viewport(
                cmd,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: target.extent.width as f32,
                    height: target.extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            device.cmd_set_scissor(cmd, 0, &[vk::Rect2D { offset: vk::Offset2D::default(), extent: target.extent }]);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                0,
                &[self.ubo_set, self.white.set],
                &[offset as u32],
            );

            if view.backdrop {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, p.backdrop);
                device.cmd_draw(cmd, 3, 1, 0, 0);
            }

            let resources = world.try_resource::<Resources>();
            let list = draw::draw_list(world, view, |asset| resources.as_ref().and_then(|r| self.imported(r, asset)));
            let bind_mesh = |m: &GpuMesh| {
                device.cmd_bind_vertex_buffers(cmd, 0, &[m.vertices.buffer], &[0]);
                device.cmd_bind_index_buffer(cmd, m.indices.buffer, 0, vk::IndexType::UINT32);
            };
            let bind_tex = |set: vk::DescriptorSet| {
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, self.layout, 1, &[set], &[]);
            };
            let mesh_for = |geometry: &Geometry| -> Option<&GpuMesh> {
                match geometry {
                    Geometry::Builtin(kind) => self.meshes.get(kind),
                    Geometry::Imported(asset) => {
                        let path = resources.as_ref()?.resolve(asset).ok()?;
                        self.imported_meshes.get(&path)?.as_ref().map(|m| &m.mesh)
                    }
                }
            };
            let pipeline_for = |pass: Pass| match pass {
                Pass::Opaque if view.wireframe => p.wire.unwrap_or(p.mesh),
                Pass::Opaque => p.mesh,
                Pass::Blended => p.mesh_blend,
                Pass::Sprite => p.sprite,
            };
            let mut current = None;
            let mut draw = |item: &draw::DrawItem| {
                let Some(mesh) = mesh_for(&item.geometry) else { return };
                let pipeline = pipeline_for(item.pass);
                if current != Some(pipeline) {
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                    current = Some(pipeline);
                }
                bind_mesh(mesh);
                bind_tex(self.texture(&item.texture));
                push(&item.uniforms);
                device.cmd_draw_indexed(cmd, item.count.unwrap_or(mesh.count), 1, item.first, 0, 0);
            };

            for item in &list.opaque {
                draw(item);
            }

            // Terrain.
            if !self.terrains.is_empty() {
                let pipeline = if view.wireframe { p.terrain_wire.unwrap_or(p.terrain) } else { p.terrain };
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                bind_tex(self.white.set);
                for t in &list.terrains {
                    let Some((_, mesh)) = self.terrains.get(&t.entity) else { continue };
                    bind_mesh(mesh);
                    push(&t.uniforms);
                    device.cmd_draw_indexed(cmd, mesh.count, 1, 0, 0, 0);
                }
            }

            // Grid (after opaque geometry so it is hidden behind objects).
            if let Some(m) = draw::grid_transform(view) {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, p.grid);
                let plane_mesh = &self.meshes[&MeshKind::Plane];
                bind_mesh(plane_mesh);
                bind_tex(self.white.set);
                push(&DrawUniforms {
                    model: m.to_cols_array_2d(),
                    color: [1.0; 4],
                    extra: [0.0; 4],
                    surface: [0.0; 4],
                    emission: [0.0; 4],
                });
                device.cmd_draw_indexed(cmd, plane_mesh.count, 1, 0, 0, 0);
            }

            // Sprites and translucent model parts, back to front.
            let mut current = None;
            for item in &list.blended {
                let Some(mesh) = mesh_for(&item.geometry) else { continue };
                let pipeline = pipeline_for(item.pass);
                if current != Some(pipeline) {
                    device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                    current = Some(pipeline);
                }
                bind_mesh(mesh);
                bind_tex(self.texture(&item.texture));
                push(&item.uniforms);
                device.cmd_draw_indexed(cmd, item.count.unwrap_or(mesh.count), 1, item.first, 0, 0);
            }

            device.cmd_end_rendering(cmd);
        }
        Ok(())
    }

    pub fn destroy(&mut self, gfx: &Gfx) {
        gfx.wait_idle();
        unsafe {
            for (_, p) in self.pipelines.drain() {
                for pl in [
                    Some(p.terrain),
                    p.terrain_wire,
                    Some(p.mesh),
                    Some(p.mesh_blend),
                    p.wire,
                    Some(p.sprite),
                    Some(p.grid),
                    Some(p.backdrop),
                ]
                .into_iter()
                .flatten()
                {
                    gfx.device.destroy_pipeline(pl, None);
                }
            }
            for (_, (_, mut m)) in self.terrains.drain() {
                m.vertices.destroy(gfx);
                m.indices.destroy(gfx);
            }
            for mut imported in self.imported_meshes.drain().filter_map(|(_, mesh)| mesh) {
                imported.mesh.vertices.destroy(gfx);
                imported.mesh.indices.destroy(gfx);
            }
            for (_, mut m) in self.meshes.drain() {
                m.vertices.destroy(gfx);
                m.indices.destroy(gfx);
            }
            for (_, mut t) in self.textures.drain() {
                t.image.destroy(gfx);
            }
            self.white.image.destroy(gfx);
            for mut a in self.attachments.drain(..) {
                if let Some(mut c) = a.color.take() {
                    c.destroy(gfx);
                }
                a.depth.destroy(gfx);
            }
            self.ubo.destroy(gfx);
            gfx.device.destroy_sampler(self.sampler, None);
            gfx.device.destroy_descriptor_pool(self.pool, None);
            gfx.device.destroy_pipeline_layout(self.layout, None);
            gfx.device.destroy_descriptor_set_layout(self.set0_layout, None);
            gfx.device.destroy_descriptor_set_layout(self.set1_layout, None);
            self.shader.destroy(gfx);
        }
    }
}
