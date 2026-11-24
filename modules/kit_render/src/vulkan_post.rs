//! Vulkan backend for `kit_post`: runs the shared pass list with Vulkan
//! textures, pipelines and descriptor sets.
//!
//! The scene is drawn into a linear RGBA16F target (MSAA resolved as usual),
//! its depth buffer is kept (multisampled; the shader reads sample 0), then
//! every pass is a fullscreen triangle into a cached intermediate target or
//! the real output. Targets are cached per output size and reused.

use super::*;
use anyhow::Context;
use kit_post::{PassUniforms, PostFrame, PostUniforms, Slot, TEMPS};
use std::ffi::CString;

/// Linear HDR format for the scene and intermediate targets.
pub(super) const HDR: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
const MAX_PASSES: usize = 32;
const PASS_STRIDE: u64 = 256;
const POST_STRIDE: u64 = (std::mem::size_of::<PostUniforms>() as u64).div_ceil(256) * 256;
const VIEW_SLOTS: usize = FRAMES * MAX_VIEWS_PER_FRAME;

struct Targets {
    extent: vk::Extent2D,
    hdr: Image,
    temps: Vec<Option<Image>>,
}

pub(super) struct PostGpu {
    samples: vk::SampleCountFlags,
    shader: ShaderModule,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pool: vk::DescriptorPool,
    sets: Vec<vk::DescriptorSet>,
    sampler: vk::Sampler,
    post_ubo: Buffer,
    pass_ubo: Buffer,
    pipelines: HashMap<(&'static str, vk::Format), vk::Pipeline>,
    targets: Vec<Targets>,
    placeholder: Image,
}

impl PostGpu {
    fn new(gfx: &Gfx, samples: vk::SampleCountFlags) -> Result<Self> {
        let device = &gfx.device;
        let source = kit_post::shader_source(samples.as_raw());
        let shader = kit_graphics::compile_wgsl(gfx, &source, "kit post")?;
        let stages = vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::VERTEX;
        let binding = |b: u32, ty: vk::DescriptorType| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(b)
                .descriptor_type(ty)
                .descriptor_count(1)
                .stage_flags(stages)
        };
        let bindings = [
            binding(0, vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC),
            binding(1, vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC),
            binding(2, vk::DescriptorType::SAMPLER),
            binding(3, vk::DescriptorType::SAMPLED_IMAGE),
            binding(4, vk::DescriptorType::SAMPLED_IMAGE),
            binding(5, vk::DescriptorType::SAMPLED_IMAGE),
        ];
        let total = (VIEW_SLOTS * MAX_PASSES) as u32;
        unsafe {
            let set_layout = device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?;
            let layout = device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(std::slice::from_ref(&set_layout)),
                None,
            )?;
            let pool = device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default().max_sets(total).pool_sizes(&[
                    vk::DescriptorPoolSize {
                        ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
                        descriptor_count: total * 2,
                    },
                    vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: total },
                    vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: total * 3 },
                ]),
                None,
            )?;
            let layouts = vec![set_layout; total as usize];
            let sets = device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default().descriptor_pool(pool).set_layouts(&layouts),
            )?;
            let sampler = device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                None,
            )?;
            let post_ubo = Buffer::new(
                gfx,
                POST_STRIDE * VIEW_SLOTS as u64,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                MemoryLocation::CpuToGpu,
                "post uniforms",
            )?;
            let pass_ubo = Buffer::new(
                gfx,
                PASS_STRIDE * (VIEW_SLOTS * MAX_PASSES) as u64,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                MemoryLocation::CpuToGpu,
                "post pass uniforms",
            )?;
            let placeholder = Image::new(
                gfx,
                vk::Extent2D { width: 1, height: 1 },
                HDR,
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                vk::SampleCountFlags::TYPE_1,
                "post placeholder",
            )?;
            gfx.one_shot(|cmd| {
                gfx.image_barrier(
                    cmd,
                    placeholder.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    (vk::PipelineStageFlags::TOP_OF_PIPE, vk::AccessFlags::empty()),
                    (vk::PipelineStageFlags::FRAGMENT_SHADER, vk::AccessFlags::SHADER_READ),
                );
            })?;
            Ok(Self {
                samples,
                shader,
                set_layout,
                layout,
                pool,
                sets,
                sampler,
                post_ubo,
                pass_ubo,
                pipelines: HashMap::new(),
                targets: Vec::new(),
                placeholder,
            })
        }
    }

    fn pipeline(&mut self, gfx: &Gfx, entry: &'static str, format: vk::Format) -> Result<vk::Pipeline> {
        if let Some(p) = self.pipelines.get(&(entry, format)) {
            return Ok(*p);
        }
        let fs = CString::new(entry)?;
        let p = kit_graphics::create_pipeline(
            gfx,
            &PipelineDesc {
                shader: &self.shader,
                vs_entry: c"vs_post",
                fs_entry: &fs,
                layout: self.layout,
                vertex: None,
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                cull: vk::CullModeFlags::NONE,
                polygon: vk::PolygonMode::FILL,
                depth_test: false,
                depth_write: false,
                blend: Blend::None,
                color_format: format,
                depth_format: None,
                samples: vk::SampleCountFlags::TYPE_1,
            },
        )
        .with_context(|| format!("post pass {entry}"))?;
        self.pipelines.insert((entry, format), p);
        Ok(p)
    }

    /// Targets for an output size, creating the temps this frame uses.
    fn targets(&mut self, gfx: &Gfx, extent: vk::Extent2D, frame: &PostFrame) -> Result<usize> {
        let index = match self.targets.iter().position(|t| t.extent == extent) {
            Some(i) => i,
            None => {
                // Keep a few sizes (main view + previews); drop the oldest.
                if self.targets.len() >= 3 {
                    gfx.wait_idle();
                    let mut old = self.targets.remove(0);
                    old.hdr.destroy(gfx);
                    for t in old.temps.iter_mut().flatten() {
                        t.destroy(gfx);
                    }
                }
                let hdr = Image::new(
                    gfx,
                    extent,
                    HDR,
                    vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
                    vk::SampleCountFlags::TYPE_1,
                    "post scene",
                )?;
                self.targets.push(Targets { extent, hdr, temps: (0..TEMPS.len()).map(|_| None).collect() });
                self.targets.len() - 1
            }
        };
        let t = &mut self.targets[index];
        for pass in &frame.passes {
            for slot in [Some(pass.output), pass.a, pass.b].into_iter().flatten() {
                if let Slot::Temp(i) = slot {
                    if t.temps[i].is_none() {
                        let (w, h) = kit_post::temp_size(i, extent.width, extent.height);
                        t.temps[i] = Some(Image::new(
                            gfx,
                            vk::Extent2D { width: w, height: h },
                            HDR,
                            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
                            vk::SampleCountFlags::TYPE_1,
                            TEMPS[i].0,
                        )?);
                    }
                }
            }
        }
        Ok(index)
    }

    pub(super) fn destroy(&mut self, gfx: &Gfx) {
        unsafe {
            for (_, p) in self.pipelines.drain() {
                gfx.device.destroy_pipeline(p, None);
            }
            for mut t in self.targets.drain(..) {
                t.hdr.destroy(gfx);
                for i in t.temps.iter_mut().flatten() {
                    i.destroy(gfx);
                }
            }
            self.placeholder.destroy(gfx);
            self.post_ubo.destroy(gfx);
            self.pass_ubo.destroy(gfx);
            gfx.device.destroy_sampler(self.sampler, None);
            gfx.device.destroy_descriptor_pool(self.pool, None);
            gfx.device.destroy_pipeline_layout(self.layout, None);
            gfx.device.destroy_descriptor_set_layout(self.set_layout, None);
            self.shader.destroy(gfx);
        }
    }
}

impl Renderer {
    /// Draw the scene into linear HDR, then run the post passes into `target`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_post(
        &mut self,
        gfx: &Gfx,
        cmd: vk::CommandBuffer,
        slot: usize,
        target: &Target,
        view: &View,
        world: &World,
        frame: &PostFrame,
    ) -> Result<()> {
        if self.post.as_ref().is_some_and(|p| p.samples != self.samples) {
            gfx.wait_idle();
            if let Some(mut old) = self.post.take() {
                old.destroy(gfx);
            }
        }
        if self.post.is_none() {
            self.post = Some(PostGpu::new(gfx, self.samples)?);
        }
        let extent = target.extent;
        let ti = self.post.as_mut().unwrap().targets(gfx, extent, frame)?;
        let (hdr_image, hdr_view) = {
            let t = &self.post.as_ref().unwrap().targets[ti];
            (t.hdr.image, t.hdr.view)
        };
        gfx.image_barrier(
            cmd,
            hdr_image,
            vk::ImageAspectFlags::COLOR,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            (vk::PipelineStageFlags::FRAGMENT_SHADER, vk::AccessFlags::SHADER_READ),
            (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
        );
        let scene_target = Target { image: hdr_image, view: hdr_view, extent, format: HDR };
        let att = self.draw_scene(gfx, cmd, slot, &scene_target, view, world, frame.needs_depth())?;
        let (depth_image, depth_view) = (self.attachments[att].depth.image, self.attachments[att].depth.view);
        self.run_passes(gfx, cmd, slot, target, frame, ti, (hdr_image, hdr_view), (depth_image, depth_view))
    }

    #[allow(clippy::too_many_arguments)]
    fn run_passes(
        &mut self,
        gfx: &Gfx,
        cmd: vk::CommandBuffer,
        slot: usize,
        target: &Target,
        frame: &PostFrame,
        ti: usize,
        hdr: (vk::Image, vk::ImageView),
        depth: (vk::Image, vk::ImageView),
    ) -> Result<()> {
        let device = &gfx.device;
        let post = self.post.as_mut().unwrap();
        anyhow::ensure!(frame.passes.len() <= MAX_PASSES, "too many post passes");
        // Scene color and depth become shader inputs.
        let read = (vk::PipelineStageFlags::FRAGMENT_SHADER, vk::AccessFlags::SHADER_READ);
        gfx.image_barrier(
            cmd,
            hdr.0,
            vk::ImageAspectFlags::COLOR,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
            read,
        );
        gfx.image_barrier(
            cmd,
            depth.0,
            vk::ImageAspectFlags::DEPTH,
            vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            (vk::PipelineStageFlags::LATE_FRAGMENT_TESTS, vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE),
            read,
        );
        let post_offset = slot as u64 * POST_STRIDE;
        post.post_ubo.write(post_offset as usize, std::slice::from_ref(&frame.uniforms));
        let view_of = |post: &PostGpu, s: Slot| -> vk::ImageView {
            match s {
                Slot::Scene => hdr.1,
                Slot::Out => target.view,
                Slot::Temp(i) => post.targets[ti].temps[i].as_ref().map_or(post.placeholder.view, |t| t.view),
            }
        };
        for (i, pass) in frame.passes.iter().enumerate() {
            let (image, image_view, format, extent) = match pass.output {
                Slot::Out => (target.image, target.view, target.format, target.extent),
                Slot::Temp(t) => {
                    let img = post.targets[ti].temps[t].as_ref().context("post target missing")?;
                    (img.image, img.view, HDR, img.extent)
                }
                Slot::Scene => anyhow::bail!("post passes can't write the scene"),
            };
            let is_temp = matches!(pass.output, Slot::Temp(_));
            if is_temp {
                gfx.image_barrier(
                    cmd,
                    image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    read,
                    (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
                );
            }
            let pipeline = post.pipeline(gfx, pass.entry, format)?;
            let index = slot * MAX_PASSES + i;
            let pass_offset = index as u64 * PASS_STRIDE;
            post.pass_ubo.write(pass_offset as usize, std::slice::from_ref::<PassUniforms>(&pass.uniforms));
            let set = post.sets[index];
            let a = pass.a.map_or(post.placeholder.view, |s| view_of(post, s));
            let b = pass.b.map_or(post.placeholder.view, |s| view_of(post, s));
            let buffers = [
                vk::DescriptorBufferInfo::default()
                    .buffer(post.post_ubo.buffer)
                    .range(std::mem::size_of::<PostUniforms>() as u64),
                vk::DescriptorBufferInfo::default().buffer(post.pass_ubo.buffer).range(PASS_STRIDE),
            ];
            let sampler = [vk::DescriptorImageInfo::default().sampler(post.sampler)];
            let image_info = |v: vk::ImageView| {
                [vk::DescriptorImageInfo::default()
                    .image_view(v)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)]
            };
            let (ia, ib, id) = (image_info(a), image_info(b), image_info(depth.1));
            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                    .buffer_info(&buffers[0..1]),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                    .buffer_info(&buffers[1..2]),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(2)
                    .descriptor_type(vk::DescriptorType::SAMPLER)
                    .image_info(&sampler),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(3)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&ia),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(4)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&ib),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(5)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&id),
            ];
            let attachment = [vk::RenderingAttachmentInfo::default()
                .image_view(image_view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::DONT_CARE)
                .store_op(vk::AttachmentStoreOp::STORE)];
            let area = vk::Rect2D { offset: vk::Offset2D::default(), extent };
            unsafe {
                device.update_descriptor_sets(&writes, &[]);
                device.cmd_begin_rendering(
                    cmd,
                    &vk::RenderingInfo::default().render_area(area).layer_count(1).color_attachments(&attachment),
                );
                device.cmd_set_viewport(
                    cmd,
                    0,
                    &[vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: extent.width as f32,
                        height: extent.height as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                    }],
                );
                device.cmd_set_scissor(cmd, 0, &[area]);
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    post.layout,
                    0,
                    &[set],
                    &[post_offset as u32, pass_offset as u32],
                );
                device.cmd_draw(cmd, 3, 1, 0, 0);
                device.cmd_end_rendering(cmd);
            }
            if is_temp {
                gfx.image_barrier(
                    cmd,
                    image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
                    read,
                );
            }
        }
        Ok(())
    }

    /// Test hook: draw the scene, then write its view depth ÷ `scale` (from
    /// the MSAA depth buffer) into `target`. Proves depth can be sampled.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn render_debug_depth(
        &mut self,
        gfx: &Gfx,
        cmd: vk::CommandBuffer,
        target: &Target,
        view: &View,
        world: &World,
        scale: f32,
    ) -> Result<()> {
        let frame = kit_post::debug_depth_frame(
            &crate::camera_info(view, false),
            target.extent.width,
            target.extent.height,
            scale,
        );
        self.render_post(gfx, cmd, 0, target, view, world, &frame)
    }
}
