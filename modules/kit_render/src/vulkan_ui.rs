//! Interfaces (`.xui`) on top of the scene: draws the [`UiFrame`] that
//! `kit_xui` tessellates each frame (the same triangles the editor shows).

use super::{FRAMES, Renderer, Target, Texture};
use anyhow::Result;
use kit_ecs::World;
use kit_graphics::{Blend, Buffer, Gfx, Image, MemoryLocation, PipelineDesc, ShaderModule, VertexLayout, vk};
use kit_xui::runtime::{UiFrame, UiTexture, UiVertex};
use std::collections::HashMap;

const SHADER: &str = r#"
struct UiPush { screen: vec2<f32>, flags: u32, pad: u32 };
var<push_constant> pc: UiPush;
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lower = c / 12.92;
    let higher = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(higher, lower, c < vec3<f32>(0.04045));
}

fn to_gamma(c: vec3<f32>) -> vec3<f32> {
    let lower = c * 12.92;
    let higher = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(higher, lower, c < vec3<f32>(0.0031308));
}

@vertex
fn vs_ui(@location(0) p: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) c: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(p.x / pc.screen.x * 2.0 - 1.0, p.y / pc.screen.y * 2.0 - 1.0, 0.0, 1.0);
    o.uv = uv;
    o.color = vec4<f32>(to_linear(c.rgb), c.a);
    return o;
}

@fragment
fn fs_ui(i: VOut) -> @location(0) vec4<f32> {
    var t = textureSample(tex, smp, i.uv);
    // Project images have straight alpha; the font atlas is premultiplied.
    if ((pc.flags & 2u) != 0u) {
        t = vec4<f32>(t.rgb * t.a, t.a);
    }
    var c = i.color * t;
    // Non-sRGB targets want gamma-encoded output.
    if ((pc.flags & 1u) != 0u) {
        c = vec4<f32>(to_gamma(c.rgb), c.a);
    }
    return c;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct UiPush {
    screen: [f32; 2],
    flags: u32,
    pad: u32,
}

pub(crate) struct UiGpu {
    shader: ShaderModule,
    layout: vk::PipelineLayout,
    pipelines: HashMap<vk::Format, vk::Pipeline>,
    font: Option<(u64, Texture)>,
    buffers: Vec<Option<(Buffer, Buffer)>>,
}

fn is_srgb(format: vk::Format) -> bool {
    matches!(format, vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB | vk::Format::A8B8G8R8_SRGB_PACK32)
}

impl Renderer {
    fn ui_gpu(&mut self, gfx: &Gfx) -> Result<&mut UiGpu> {
        if self.ui.is_none() {
            let shader = kit_graphics::compile_wgsl(gfx, SHADER, "kit ui")?;
            let push = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .size(std::mem::size_of::<UiPush>() as u32)];
            let layouts = [self.set1_layout];
            let layout = unsafe {
                gfx.device.create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts).push_constant_ranges(&push),
                    None,
                )?
            };
            self.ui = Some(UiGpu {
                shader,
                layout,
                pipelines: HashMap::new(),
                font: None,
                buffers: (0..FRAMES).map(|_| None).collect(),
            });
        }
        Ok(self.ui.as_mut().unwrap())
    }

    /// Upload the font atlas when it changed.
    pub(crate) fn prepare_ui(&mut self, gfx: &Gfx, world: &World) {
        let Some(frame) = world.try_resource::<UiFrame>().map(|f| f.atlas.clone()) else { return };
        if frame.rgba.is_empty() {
            return;
        }
        let current = self.ui.as_ref().and_then(|u| u.font.as_ref().map(|(r, _)| *r));
        if current == Some(frame.revision) {
            return;
        }
        let made =
            Image::texture_rgba8(gfx, frame.size[0] as u32, frame.size[1] as u32, &frame.rgba, true, "ui font atlas")
                .and_then(|image| Ok(Texture { set: self.texture_set(gfx, image.view)?, image, modified: None }));
        match made {
            Ok(tex) => {
                let old = self.ui_gpu(gfx).ok().and_then(|ui| ui.font.replace((frame.revision, tex)));
                if let Some((_, old)) = old {
                    self.free_texture(gfx, old);
                }
            }
            Err(e) => log::error!("ui font atlas: {e:#}"),
        }
    }

    /// Draw this frame's interface over `target` (after [`Renderer::render`]).
    pub fn draw_ui(
        &mut self,
        gfx: &Gfx,
        cmd: vk::CommandBuffer,
        frame_index: usize,
        target: &Target,
        world: &World,
    ) -> Result<()> {
        let Some(frame) = world.try_resource::<UiFrame>().map(|f| f.clone()) else { return Ok(()) };
        if frame.meshes.is_empty() || target.extent.width == 0 || target.extent.height == 0 {
            return Ok(());
        }
        let format = target.format;
        let (vcount, icount) =
            frame.meshes.iter().fold((0, 0), |(v, i), m| (v + m.vertices.len(), i + m.indices.len()));
        let slot = frame_index % FRAMES;
        // Textures per mesh (resolved before borrowing the UI state).
        let sets: Vec<(vk::DescriptorSet, bool)> = frame
            .meshes
            .iter()
            .map(|m| match &m.texture {
                UiTexture::Font => (
                    self.ui.as_ref().and_then(|u| u.font.as_ref().map(|(_, t)| t.set)).unwrap_or(self.white.set),
                    false,
                ),
                UiTexture::Image(path) => (self.texture(path), true),
            })
            .collect();
        let ui = self.ui_gpu(gfx)?;
        if !ui.pipelines.contains_key(&format) {
            let pipeline = kit_graphics::create_pipeline(
                gfx,
                &PipelineDesc {
                    shader: &ui.shader,
                    vs_entry: c"vs_ui",
                    fs_entry: c"fs_ui",
                    layout: ui.layout,
                    vertex: Some(VertexLayout {
                        stride: std::mem::size_of::<UiVertex>() as u32,
                        attributes: vec![
                            (0, vk::Format::R32G32_SFLOAT, 0),
                            (1, vk::Format::R32G32_SFLOAT, 8),
                            (2, vk::Format::R8G8B8A8_UNORM, 16),
                        ],
                    }),
                    topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                    cull: vk::CullModeFlags::NONE,
                    polygon: vk::PolygonMode::FILL,
                    depth_test: false,
                    depth_write: false,
                    blend: Blend::Premultiplied,
                    color_format: format,
                    depth_format: None,
                    samples: vk::SampleCountFlags::TYPE_1,
                },
            )?;
            ui.pipelines.insert(format, pipeline);
        }
        let vbytes = (vcount * std::mem::size_of::<UiVertex>()) as u64;
        let ibytes = (icount * 4) as u64;
        let fits = ui.buffers[slot].as_ref().is_some_and(|(v, i)| v.size >= vbytes && i.size >= ibytes);
        if !fits {
            if let Some((mut v, mut i)) = ui.buffers[slot].take() {
                v.destroy(gfx);
                i.destroy(gfx);
            }
            let grow = |n: u64| n.next_power_of_two().max(64 * 1024);
            let v = Buffer::new(
                gfx,
                grow(vbytes),
                vk::BufferUsageFlags::VERTEX_BUFFER,
                MemoryLocation::CpuToGpu,
                "ui vertices",
            )?;
            let i = Buffer::new(
                gfx,
                grow(ibytes),
                vk::BufferUsageFlags::INDEX_BUFFER,
                MemoryLocation::CpuToGpu,
                "ui indices",
            )?;
            ui.buffers[slot] = Some((v, i));
        }
        let (vb, ib) = ui.buffers[slot].as_mut().unwrap();
        let mut voff = 0usize;
        let mut ioff = 0usize;
        let mut draws = Vec::with_capacity(frame.meshes.len());
        for m in &frame.meshes {
            vb.write(voff * std::mem::size_of::<UiVertex>(), &m.vertices);
            ib.write(ioff * 4, &m.indices);
            draws.push((voff as i32, ioff as u32, m.indices.len() as u32));
            voff += m.vertices.len();
            ioff += m.indices.len();
        }
        let (vbuf, ibuf) = (vb.buffer, ib.buffer);
        let pipeline = ui.pipelines[&format];
        let layout = ui.layout;
        let device = &gfx.device;
        let ppp = frame.pixels_per_point.max(0.1);
        let gamma = u32::from(!is_srgb(format));
        // The scene pass wrote this image; wait for it before drawing over it.
        gfx.image_barrier(
            cmd,
            target.image,
            vk::ImageAspectFlags::COLOR,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
            (
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            ),
        );
        unsafe {
            let color = [vk::RenderingAttachmentInfo::default()
                .image_view(target.view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::LOAD)
                .store_op(vk::AttachmentStoreOp::STORE)];
            let area = vk::Rect2D { offset: vk::Offset2D::default(), extent: target.extent };
            device.cmd_begin_rendering(
                cmd,
                &vk::RenderingInfo::default().render_area(area).layer_count(1).color_attachments(&color),
            );
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
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
            device.cmd_bind_vertex_buffers(cmd, 0, &[vbuf], &[0]);
            device.cmd_bind_index_buffer(cmd, ibuf, 0, vk::IndexType::UINT32);
            // Points → target pixels (the target may differ from the window size, e.g. screenshots).
            let sx = target.extent.width as f32 / (frame.size[0] * ppp).max(1.0) * ppp;
            let sy = target.extent.height as f32 / (frame.size[1] * ppp).max(1.0) * ppp;
            for ((m, (set, straight)), (vo, io, n)) in frame.meshes.iter().zip(&sets).zip(&draws) {
                let x0 = (m.clip[0] * sx).floor().clamp(0.0, target.extent.width as f32);
                let y0 = (m.clip[1] * sy).floor().clamp(0.0, target.extent.height as f32);
                let x1 = (m.clip[2] * sx).ceil().clamp(0.0, target.extent.width as f32);
                let y1 = (m.clip[3] * sy).ceil().clamp(0.0, target.extent.height as f32);
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                device.cmd_set_scissor(
                    cmd,
                    0,
                    &[vk::Rect2D {
                        offset: vk::Offset2D { x: x0 as i32, y: y0 as i32 },
                        extent: vk::Extent2D { width: (x1 - x0) as u32, height: (y1 - y0) as u32 },
                    }],
                );
                device.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, layout, 0, &[*set], &[]);
                let push = UiPush { screen: frame.size, flags: gamma | if *straight { 2 } else { 0 }, pad: 0 };
                device.cmd_push_constants(
                    cmd,
                    layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    bytemuck::bytes_of(&push),
                );
                device.cmd_draw_indexed(cmd, *n, 1, *io, *vo, 0);
            }
            device.cmd_end_rendering(cmd);
        }
        Ok(())
    }

    pub(crate) fn destroy_ui(&mut self, gfx: &Gfx) {
        let Some(mut ui) = self.ui.take() else { return };
        unsafe {
            for (_, p) in ui.pipelines.drain() {
                gfx.device.destroy_pipeline(p, None);
            }
            gfx.device.destroy_pipeline_layout(ui.layout, None);
        }
        for (mut v, mut i) in ui.buffers.iter_mut().filter_map(Option::take) {
            v.destroy(gfx);
            i.destroy(gfx);
        }
        ui.shader.destroy(gfx);
        if let Some((_, t)) = ui.font.take() {
            self.free_texture(gfx, t);
        }
    }
}
