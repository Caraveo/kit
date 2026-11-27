//! Shader compilation (WGSL → SPIR-V via naga) and graphics pipelines.

use crate::{Gfx, vk};
use anyhow::{Result, anyhow};

/// A compiled shader module. All entry points of the WGSL source live in it.
pub struct ShaderModule {
    pub module: vk::ShaderModule,
}

impl ShaderModule {
    pub fn destroy(&mut self, gfx: &Gfx) {
        unsafe { gfx.device.destroy_shader_module(self.module, None) };
        self.module = vk::ShaderModule::null();
    }
}

/// Compile WGSL source to a Vulkan shader module.
///
/// Push constants are declared as `var<immediate> pc: MyStruct;`.
/// Projection matrices are expected to be Vulkan-style (Y down), so no
/// coordinate flip is applied.
pub fn compile_wgsl(gfx: &Gfx, source: &str, label: &str) -> Result<ShaderModule> {
    let module =
        naga::front::wgsl::parse_str(source).map_err(|e| anyhow!("shader {label}: {}", e.emit_to_string(source)))?;
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::IMMEDIATES)
        .validate(&module)
        .map_err(|e| anyhow!("shader {label}: {}", e.emit_to_string(source)))?;
    let mut options = naga::back::spv::Options::default();
    options.lang_version = (1, 3);
    options.flags.remove(naga::back::spv::WriterFlags::ADJUST_COORDINATE_SPACE);
    let words = naga::back::spv::write_vec(&module, &info, &options, None)
        .map_err(|e| anyhow!("shader {label}: SPIR-V generation failed: {e}"))?;
    let vk_module =
        unsafe { gfx.device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)? };
    Ok(ShaderModule { module: vk_module })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// Opaque, no blending.
    None,
    /// Premultiplied-style "over" blending with straight alpha.
    Alpha,
    /// "Over" blending for colors that are already premultiplied by alpha (UI).
    Premultiplied,
}

/// Vertex buffer layout: one interleaved binding.
#[derive(Clone, Debug, Default)]
pub struct VertexLayout {
    pub stride: u32,
    /// (location, format, offset)
    pub attributes: Vec<(u32, vk::Format, u32)>,
}

/// Everything needed to build a graphics pipeline for dynamic rendering.
pub struct PipelineDesc<'a> {
    pub shader: &'a ShaderModule,
    pub vs_entry: &'a std::ffi::CStr,
    pub fs_entry: &'a std::ffi::CStr,
    pub layout: vk::PipelineLayout,
    /// `None` for pipelines that generate vertices in the shader (fullscreen passes).
    pub vertex: Option<VertexLayout>,
    pub topology: vk::PrimitiveTopology,
    pub cull: vk::CullModeFlags,
    pub polygon: vk::PolygonMode,
    pub depth_test: bool,
    pub depth_write: bool,
    pub blend: Blend,
    pub color_format: vk::Format,
    pub depth_format: Option<vk::Format>,
    pub samples: vk::SampleCountFlags,
}

pub fn create_pipeline(gfx: &Gfx, d: &PipelineDesc) -> Result<vk::Pipeline> {
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(d.shader.module)
            .name(d.vs_entry),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(d.shader.module)
            .name(d.fs_entry),
    ];
    let (bindings, attributes): (Vec<_>, Vec<_>) = match &d.vertex {
        Some(v) => (
            vec![
                vk::VertexInputBindingDescription::default()
                    .binding(0)
                    .stride(v.stride)
                    .input_rate(vk::VertexInputRate::VERTEX),
            ],
            v.attributes
                .iter()
                .map(|(loc, fmt, off)| {
                    vk::VertexInputAttributeDescription::default().location(*loc).binding(0).format(*fmt).offset(*off)
                })
                .collect(),
        ),
        None => (Vec::new(), Vec::new()),
    };
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&bindings)
        .vertex_attribute_descriptions(&attributes);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(d.topology);
    let viewport = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(d.polygon)
        .cull_mode(d.cull)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(d.samples);
    let depth = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(d.depth_test)
        .depth_write_enable(d.depth_write)
        .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
    let attachment = match d.blend {
        Blend::None => vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(false),
        Blend::Alpha => vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD),
        Blend::Premultiplied => vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD),
    };
    let attachments = [attachment];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let color_formats = [d.color_format];
    let mut rendering = vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&color_formats);
    if let Some(df) = d.depth_format {
        rendering = rendering.depth_attachment_format(df);
    }
    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .depth_stencil_state(&depth)
        .color_blend_state(&blend)
        .dynamic_state(&dynamic)
        .layout(d.layout)
        .push_next(&mut rendering);
    let pipelines = unsafe {
        gfx.device
            .create_graphics_pipelines(vk::PipelineCache::null(), &[info], None)
            .map_err(|(_, e)| anyhow!("creating pipeline: {e}"))?
    };
    Ok(pipelines[0])
}

#[cfg(test)]
mod builtin_shader_tests {
    /// Kit's built-in shader (shared by every renderer) must always compile:
    /// for Vulkan, and as the web player rewrites it for WebGPU.
    #[test]
    fn the_builtin_shader_validates_for_desktop_and_web() {
        let source = include_str!("../../kit_render/src/shaders.wgsl");
        let web = source.replace("var<immediate> pc: Push;", "@group(2) @binding(0) var<uniform> pc: Push;");
        for (label, src, caps) in [
            ("desktop", source.to_owned(), naga::valid::Capabilities::IMMEDIATES),
            ("web", web, naga::valid::Capabilities::empty()),
        ] {
            let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{label}: {}", e.emit_to_string(&src)));
            let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps)
                .validate(&module)
                .unwrap_or_else(|e| panic!("{label}: {}", e.emit_to_string(&src)));
            let mut options = naga::back::spv::Options::default();
            options.lang_version = (1, 3);
            naga::back::spv::write_vec(&module, &info, &options, None).unwrap_or_else(|e| panic!("{label}: {e}"));
        }
    }
}
