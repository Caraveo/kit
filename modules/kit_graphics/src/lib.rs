//! # kit_graphics
//!
//! Kit's Vulkan layer (Defold's `graphics` module): instance and device setup,
//! swapchain presentation, GPU buffers and images, shader compilation and
//! pipeline creation. Higher-level drawing lives in `kit_render`.
//!
//! Requirements: Vulkan 1.3 (dynamic rendering). On macOS this runs on MoltenVK
//! through the portability extensions.

mod pipeline;
mod resources;
mod swapchain;

pub use ash::{self, vk};
pub use gpu_allocator::MemoryLocation;
pub use pipeline::{Blend, PipelineDesc, ShaderModule, VertexLayout, compile_wgsl, create_pipeline};
pub use resources::{Buffer, Image};
pub use swapchain::{FRAMES_IN_FLIGHT, FrameInfo, Presenter};

use anyhow::{Context, Result, anyhow, bail};
use ash::{ext, khr};
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::ffi::{CStr, c_char};
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};

pub const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;

/// Optional device capabilities Kit can use when present.
#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities {
    /// Wireframe rendering (`fillModeNonSolid`).
    pub wireframe: bool,
    /// MSAA sample count used for scene rendering.
    pub msaa: vk::SampleCountFlags,
}

/// The Vulkan context shared by everything that renders.
pub struct Gfx {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    debug: Option<(ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    pub surface_fn: Option<khr::surface::Instance>,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    pub swapchain_fn: Option<khr::swapchain::Device>,
    pub queue: vk::Queue,
    pub queue_family: u32,
    allocator: ManuallyDrop<Arc<Mutex<Allocator>>>,
    /// Pool for one-shot upload command buffers.
    pub upload_pool: vk::CommandPool,
    pub caps: Capabilities,
    pub device_name: String,
    pub api_version: u32,
}

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _ty: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let msg = unsafe {
        if data.is_null() || (*data).p_message.is_null() {
            "<no message>".to_string()
        } else {
            CStr::from_ptr((*data).p_message).to_string_lossy().to_string()
        }
    };
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        log::error!("vulkan: {msg}");
    } else if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
        log::warn!("vulkan: {msg}");
    } else {
        log::debug!("vulkan: {msg}");
    }
    vk::FALSE
}

/// Find the Vulkan library. On macOS, prefer a MoltenVK bundled with the app
/// (`Kit.app/Contents/Frameworks` or next to the executable).
fn load_entry() -> Result<ash::Entry> {
    if cfg!(target_os = "macos") {
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
            for candidate in [
                dir.join("../Frameworks/libvulkan.1.dylib"),
                dir.join("libvulkan.1.dylib"),
                dir.join("../Frameworks/libMoltenVK.dylib"),
                dir.join("libMoltenVK.dylib"),
            ] {
                if candidate.is_file() {
                    if let Ok(entry) = unsafe { ash::Entry::load_from(&candidate) } {
                        log::info!("Vulkan: {}", candidate.display());
                        return Ok(entry);
                    }
                }
            }
        }
    }
    unsafe { ash::Entry::load() }.map_err(|e| {
        anyhow!("Vulkan was not found ({e}). Install your GPU driver's Vulkan support (or MoltenVK on macOS).")
    })
}

fn has_ext(list: &[vk::ExtensionProperties], name: &CStr) -> bool {
    list.iter().any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name))
}

impl Gfx {
    /// Create the Vulkan context. Pass the display handle to enable presentation
    /// (window surfaces); `None` creates a headless context.
    ///
    /// Validation layers are enabled when `KIT_VALIDATION=1`, or in debug builds
    /// when the layer is installed.
    pub fn new(display: Option<RawDisplayHandle>, app_name: &str) -> Result<Self> {
        let entry = load_entry()?;

        let instance_api = unsafe { entry.try_enumerate_instance_version()? }.unwrap_or(vk::API_VERSION_1_0);
        if instance_api < vk::API_VERSION_1_3 {
            bail!(
                "Kit needs Vulkan 1.3, but the installed loader is {}.{}",
                vk::api_version_major(instance_api),
                vk::api_version_minor(instance_api)
            );
        }

        let available_ext = unsafe { entry.enumerate_instance_extension_properties(None)? };
        let available_layers = unsafe { entry.enumerate_instance_layer_properties()? };
        let validation_name = c"VK_LAYER_KHRONOS_validation";
        let validation_wanted = match std::env::var("KIT_VALIDATION").as_deref() {
            Ok("1") | Ok("true") => true,
            Ok(_) => false,
            Err(_) => cfg!(debug_assertions),
        };
        let validation = validation_wanted
            && available_layers.iter().any(|l| l.layer_name_as_c_str().is_ok_and(|n| n == validation_name));
        if validation_wanted && !validation {
            log::warn!("Vulkan validation layer not installed; continuing without it");
        }

        let mut extensions: Vec<*const c_char> = Vec::new();
        if let Some(display) = display {
            extensions.extend_from_slice(ash_window::enumerate_required_extensions(display)?);
        }
        let debug_utils = has_ext(&available_ext, ext::debug_utils::NAME);
        if validation && debug_utils {
            extensions.push(ext::debug_utils::NAME.as_ptr());
        }
        let mut flags = vk::InstanceCreateFlags::empty();
        if has_ext(&available_ext, khr::portability_enumeration::NAME) {
            extensions.push(khr::portability_enumeration::NAME.as_ptr());
            flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
        }
        if has_ext(&available_ext, khr::get_physical_device_properties2::NAME) {
            extensions.push(khr::get_physical_device_properties2::NAME.as_ptr());
        }

        let app_name_c = std::ffi::CString::new(app_name).unwrap_or_default();
        let app_info = vk::ApplicationInfo::default()
            .application_name(&app_name_c)
            .engine_name(c"Kit")
            .engine_version(vk::make_api_version(0, 0, 0, 1))
            .api_version(vk::API_VERSION_1_3);
        let layers = [validation_name.as_ptr()];
        let mut create = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&extensions)
            .flags(flags);
        if validation {
            create = create.enabled_layer_names(&layers);
        }
        let instance = unsafe { entry.create_instance(&create, None) }.context("creating Vulkan instance")?;

        let debug = if validation && debug_utils {
            let loader = ext::debug_utils::Instance::new(&entry, &instance);
            let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::ERROR | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                )
                .message_type(
                    vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                )
                .pfn_user_callback(Some(debug_callback));
            let messenger = unsafe { loader.create_debug_utils_messenger(&info, None)? };
            log::info!("Vulkan validation enabled");
            Some((loader, messenger))
        } else {
            None
        };

        let surface_fn = display.map(|_| khr::surface::Instance::new(&entry, &instance));

        // Pick a device: prefer discrete GPUs with graphics and Vulkan 1.3.
        let devices = unsafe { instance.enumerate_physical_devices()? };
        let mut best: Option<(i32, vk::PhysicalDevice, u32)> = None;
        for pd in devices {
            let props = unsafe { instance.get_physical_device_properties(pd) };
            if props.api_version < vk::API_VERSION_1_3 {
                continue;
            }
            let exts = unsafe { instance.enumerate_device_extension_properties(pd)? };
            if display.is_some() && !has_ext(&exts, khr::swapchain::NAME) {
                continue;
            }
            let families = unsafe { instance.get_physical_device_queue_family_properties(pd) };
            let Some(family) = families.iter().position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS)) else {
                continue;
            };
            let score = match props.device_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => 3,
                vk::PhysicalDeviceType::INTEGRATED_GPU => 2,
                vk::PhysicalDeviceType::VIRTUAL_GPU => 1,
                _ => 0,
            };
            if best.is_none_or(|(s, _, _)| score > s) {
                best = Some((score, pd, family as u32));
            }
        }
        let Some((_, physical, queue_family)) = best else {
            bail!("no GPU with Vulkan 1.3 and graphics support was found");
        };
        let props = unsafe { instance.get_physical_device_properties(physical) };
        let device_name = props.device_name_as_c_str().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        log::info!(
            "GPU: {device_name} (Vulkan {}.{})",
            vk::api_version_major(props.api_version),
            vk::api_version_minor(props.api_version)
        );

        let dev_exts = unsafe { instance.enumerate_device_extension_properties(physical)? };
        let mut device_ext: Vec<*const c_char> = Vec::new();
        if display.is_some() {
            device_ext.push(khr::swapchain::NAME.as_ptr());
        }
        if has_ext(&dev_exts, khr::portability_subset::NAME) {
            device_ext.push(khr::portability_subset::NAME.as_ptr());
        }

        let supported = unsafe { instance.get_physical_device_features(physical) };
        let features = vk::PhysicalDeviceFeatures::default()
            .fill_mode_non_solid(supported.fill_mode_non_solid == vk::TRUE)
            .sampler_anisotropy(supported.sampler_anisotropy == vk::TRUE);
        let mut v13 = vk::PhysicalDeviceVulkan13Features::default().dynamic_rendering(true);
        let priorities = [1.0f32];
        let queue_info =
            [vk::DeviceQueueCreateInfo::default().queue_family_index(queue_family).queue_priorities(&priorities)];
        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&device_ext)
            .enabled_features(&features)
            .push_next(&mut v13);
        let device =
            unsafe { instance.create_device(physical, &device_info, None) }.context("creating Vulkan device")?;
        let queue = unsafe { device.get_device_queue(queue_family, 0) };
        let swapchain_fn = display.map(|_| khr::swapchain::Device::new(&instance, &device));

        let allocator = Allocator::new(&AllocatorCreateDesc {
            instance: instance.clone(),
            device: device.clone(),
            physical_device: physical,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })
        .context("creating GPU allocator")?;

        let upload_pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(queue_family)
                    .flags(vk::CommandPoolCreateFlags::TRANSIENT | vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?
        };

        let counts = props.limits.framebuffer_color_sample_counts & props.limits.framebuffer_depth_sample_counts;
        let msaa = [vk::SampleCountFlags::TYPE_4, vk::SampleCountFlags::TYPE_2]
            .into_iter()
            .find(|s| counts.contains(*s))
            .unwrap_or(vk::SampleCountFlags::TYPE_1);

        Ok(Self {
            entry,
            instance,
            debug,
            surface_fn,
            physical,
            device,
            swapchain_fn,
            queue,
            queue_family,
            allocator: ManuallyDrop::new(Arc::new(Mutex::new(allocator))),
            upload_pool,
            caps: Capabilities { wireframe: supported.fill_mode_non_solid == vk::TRUE, msaa },
            device_name,
            api_version: props.api_version,
        })
    }

    /// The shared GPU memory allocator.
    pub fn allocator(&self) -> &Arc<Mutex<Allocator>> {
        &self.allocator
    }

    /// Create a presentation surface for a window.
    pub fn create_surface(&self, display: RawDisplayHandle, window: RawWindowHandle) -> Result<vk::SurfaceKHR> {
        let surface = unsafe { ash_window::create_surface(&self.entry, &self.instance, display, window, None)? };
        let supported = unsafe {
            self.surface_fn.as_ref().context("Gfx was created without a display")?.get_physical_device_surface_support(
                self.physical,
                self.queue_family,
                surface,
            )?
        };
        if !supported {
            bail!("the selected GPU queue cannot present to this window");
        }
        Ok(surface)
    }

    pub fn wait_idle(&self) {
        unsafe {
            let _ = self.device.device_wait_idle();
        }
    }

    /// Record and submit a one-off command buffer, waiting for it to finish.
    pub fn one_shot(&self, record: impl FnOnce(vk::CommandBuffer)) -> Result<()> {
        unsafe {
            let cmd = self.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.upload_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
            self.device.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            record(cmd);
            self.device.end_command_buffer(cmd)?;
            let fence = self.device.create_fence(&vk::FenceCreateInfo::default(), None)?;
            let cmds = [cmd];
            let submit = [vk::SubmitInfo::default().command_buffers(&cmds)];
            let result = self.device.queue_submit(self.queue, &submit, fence);
            if result.is_ok() {
                self.device.wait_for_fences(&[fence], true, u64::MAX)?;
            }
            self.device.destroy_fence(fence, None);
            self.device.free_command_buffers(self.upload_pool, &cmds);
            result?;
        }
        Ok(())
    }

    /// Insert an image layout transition / memory barrier.
    #[allow(clippy::too_many_arguments)]
    pub fn image_barrier(
        &self,
        cmd: vk::CommandBuffer,
        image: vk::Image,
        aspect: vk::ImageAspectFlags,
        old: vk::ImageLayout,
        new: vk::ImageLayout,
        src: (vk::PipelineStageFlags, vk::AccessFlags),
        dst: (vk::PipelineStageFlags, vk::AccessFlags),
    ) {
        let barrier = vk::ImageMemoryBarrier::default()
            .image(image)
            .old_layout(old)
            .new_layout(new)
            .src_access_mask(src.1)
            .dst_access_mask(dst.1)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .subresource_range(vk::ImageSubresourceRange::default().aspect_mask(aspect).level_count(1).layer_count(1));
        unsafe {
            self.device.cmd_pipeline_barrier(cmd, src.0, dst.0, vk::DependencyFlags::empty(), &[], &[], &[barrier]);
        }
    }
}

impl Drop for Gfx {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_command_pool(self.upload_pool, None);
            let alloc = ManuallyDrop::take(&mut self.allocator);
            if Arc::strong_count(&alloc) > 1 {
                log::warn!("GPU allocator still shared at shutdown; leaking it");
                std::mem::forget(alloc);
            } else {
                drop(alloc);
            }
            self.device.destroy_device(None);
            if let Some((loader, messenger)) = self.debug.take() {
                loader.destroy_debug_utils_messenger(messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}
