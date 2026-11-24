//! Window presentation: swapchain, frames in flight and per-frame command buffers.

use crate::{Gfx, vk};
use anyhow::{Context, Result};

/// Number of frames the CPU may record ahead of the GPU.
pub const FRAMES_IN_FLIGHT: usize = 2;

struct FrameSync {
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    image_available: vk::Semaphore,
}

/// What the caller records into for one frame.
///
/// The swapchain image arrives in `COLOR_ATTACHMENT_OPTIMAL` (contents
/// undefined) and must be left in that layout; [`Presenter::end_frame`]
/// transitions it for presentation.
pub struct FrameInfo {
    pub cmd: vk::CommandBuffer,
    /// Index of the frame-in-flight slot (0..FRAMES_IN_FLIGHT).
    pub frame_index: usize,
    pub image_index: u32,
    pub image: vk::Image,
    pub view: vk::ImageView,
    pub extent: vk::Extent2D,
    pub format: vk::Format,
}

/// Owns a window surface and its swapchain.
pub struct Presenter {
    surface: vk::SurfaceKHR,
    swapchain: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    /// One per swapchain image, so a semaphore is never reused while presenting.
    render_finished: Vec<vk::Semaphore>,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pool: vk::CommandPool,
    frames: Vec<FrameSync>,
    current: usize,
    pub vsync: bool,
    dirty: bool,
    wanted: vk::Extent2D,
}

impl Presenter {
    pub fn new(gfx: &Gfx, surface: vk::SurfaceKHR, width: u32, height: u32, vsync: bool) -> Result<Self> {
        let surface_fn = gfx.surface_fn.as_ref().context("Gfx has no surface support")?;
        let formats = unsafe { surface_fn.get_physical_device_surface_formats(gfx.physical, surface)? };
        // Prefer an sRGB format so shading happens in linear space.
        let format = formats
            .iter()
            .find(|f| {
                matches!(f.format, vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB)
                    && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            })
            .or_else(|| formats.first())
            .context("surface reports no formats")?;

        let pool = unsafe {
            gfx.device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(gfx.queue_family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?
        };
        let cmds = unsafe {
            gfx.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(FRAMES_IN_FLIGHT as u32),
            )?
        };
        let mut frames = Vec::new();
        for cmd in cmds {
            unsafe {
                frames.push(FrameSync {
                    cmd,
                    fence: gfx
                        .device
                        .create_fence(&vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED), None)?,
                    image_available: gfx.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
                });
            }
        }
        let mut p = Self {
            surface,
            swapchain: vk::SwapchainKHR::null(),
            images: Vec::new(),
            views: Vec::new(),
            render_finished: Vec::new(),
            format: format.format,
            extent: vk::Extent2D { width, height },
            pool,
            frames,
            current: 0,
            vsync,
            dirty: true,
            wanted: vk::Extent2D { width, height },
        };
        let color_space = format.color_space;
        p.create_swapchain(gfx, color_space)?;
        Ok(p)
    }

    fn create_swapchain(&mut self, gfx: &Gfx, color_space: vk::ColorSpaceKHR) -> Result<()> {
        let surface_fn = gfx.surface_fn.as_ref().unwrap();
        let swapchain_fn = gfx.swapchain_fn.as_ref().context("Gfx has no swapchain support")?;
        let caps = unsafe { surface_fn.get_physical_device_surface_capabilities(gfx.physical, self.surface)? };
        let extent = if caps.current_extent.width != u32::MAX {
            caps.current_extent
        } else {
            vk::Extent2D {
                width: self.wanted.width.clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                height: self.wanted.height.clamp(caps.min_image_extent.height, caps.max_image_extent.height),
            }
        };
        if extent.width == 0 || extent.height == 0 {
            // Minimized: keep the old swapchain until we get a real size.
            self.dirty = true;
            return Ok(());
        }
        let modes = unsafe { surface_fn.get_physical_device_surface_present_modes(gfx.physical, self.surface)? };
        let present_mode = if self.vsync {
            vk::PresentModeKHR::FIFO
        } else if modes.contains(&vk::PresentModeKHR::MAILBOX) {
            vk::PresentModeKHR::MAILBOX
        } else if modes.contains(&vk::PresentModeKHR::IMMEDIATE) {
            vk::PresentModeKHR::IMMEDIATE
        } else {
            vk::PresentModeKHR::FIFO
        };
        let mut image_count = caps.min_image_count + 1;
        if caps.max_image_count > 0 {
            image_count = image_count.min(caps.max_image_count);
        }
        let composite = [
            vk::CompositeAlphaFlagsKHR::OPAQUE,
            vk::CompositeAlphaFlagsKHR::INHERIT,
            vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
            vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
        ]
        .into_iter()
        .find(|c| caps.supported_composite_alpha.contains(*c))
        .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE);

        let old = self.swapchain;
        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(self.surface)
            .min_image_count(image_count)
            .image_format(self.format)
            .image_color_space(color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(composite)
            .present_mode(present_mode)
            .clipped(true)
            .old_swapchain(old);
        let swapchain = unsafe { swapchain_fn.create_swapchain(&info, None)? };
        self.destroy_views(gfx);
        if old != vk::SwapchainKHR::null() {
            unsafe { swapchain_fn.destroy_swapchain(old, None) };
        }
        self.swapchain = swapchain;
        self.extent = extent;
        self.images = unsafe { swapchain_fn.get_swapchain_images(swapchain)? };
        for &image in &self.images {
            let view = unsafe {
                gfx.device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(self.format)
                        .subresource_range(
                            vk::ImageSubresourceRange::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .level_count(1)
                                .layer_count(1),
                        ),
                    None,
                )?
            };
            self.views.push(view);
            self.render_finished
                .push(unsafe { gfx.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)? });
        }
        self.dirty = false;
        log::debug!("swapchain {}x{} {:?} {:?}", extent.width, extent.height, self.format, present_mode);
        Ok(())
    }

    fn destroy_views(&mut self, gfx: &Gfx) {
        unsafe {
            for v in self.views.drain(..) {
                gfx.device.destroy_image_view(v, None);
            }
            for s in self.render_finished.drain(..) {
                gfx.device.destroy_semaphore(s, None);
            }
        }
        self.images.clear();
    }

    /// Request a new size (call on window resize).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.wanted = vk::Extent2D { width, height };
        self.dirty = true;
    }

    pub fn set_vsync(&mut self, vsync: bool) {
        if self.vsync != vsync {
            self.vsync = vsync;
            self.dirty = true;
        }
    }

    fn recreate(&mut self, gfx: &Gfx) -> Result<()> {
        gfx.wait_idle();
        let surface_fn = gfx.surface_fn.as_ref().unwrap();
        let formats = unsafe { surface_fn.get_physical_device_surface_formats(gfx.physical, self.surface)? };
        let cs = formats
            .iter()
            .find(|f| f.format == self.format)
            .map(|f| f.color_space)
            .unwrap_or(vk::ColorSpaceKHR::SRGB_NONLINEAR);
        self.create_swapchain(gfx, cs)
    }

    /// Begin a frame. Returns `None` when there is nothing to draw into this
    /// time (window minimized or swapchain being recreated).
    pub fn begin_frame(&mut self, gfx: &Gfx) -> Result<Option<FrameInfo>> {
        if self.dirty {
            self.recreate(gfx)?;
            if self.dirty {
                return Ok(None);
            }
        }
        let swapchain_fn = gfx.swapchain_fn.as_ref().unwrap();
        let frame = &self.frames[self.current];
        unsafe { gfx.device.wait_for_fences(&[frame.fence], true, u64::MAX)? };
        let image_index = match unsafe {
            swapchain_fn.acquire_next_image(self.swapchain, u64::MAX, frame.image_available, vk::Fence::null())
        } {
            Ok((i, suboptimal)) => {
                if suboptimal {
                    self.dirty = true;
                }
                i
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.dirty = true;
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        unsafe {
            gfx.device.reset_fences(&[frame.fence])?;
            gfx.device.reset_command_buffer(frame.cmd, vk::CommandBufferResetFlags::empty())?;
            gfx.device.begin_command_buffer(
                frame.cmd,
                &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
        }
        let image = self.images[image_index as usize];
        gfx.image_barrier(
            frame.cmd,
            image,
            vk::ImageAspectFlags::COLOR,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::empty()),
            (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
        );
        Ok(Some(FrameInfo {
            cmd: frame.cmd,
            frame_index: self.current,
            image_index,
            image,
            view: self.views[image_index as usize],
            extent: self.extent,
            format: self.format,
        }))
    }

    /// Finish recording, submit and present.
    pub fn end_frame(&mut self, gfx: &Gfx, info: FrameInfo) -> Result<()> {
        let frame = &self.frames[self.current];
        gfx.image_barrier(
            info.cmd,
            info.image,
            vk::ImageAspectFlags::COLOR,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            vk::ImageLayout::PRESENT_SRC_KHR,
            (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
            (vk::PipelineStageFlags::BOTTOM_OF_PIPE, vk::AccessFlags::empty()),
        );
        let render_finished = self.render_finished[info.image_index as usize];
        unsafe {
            gfx.device.end_command_buffer(info.cmd)?;
            let wait = [frame.image_available];
            let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let cmds = [info.cmd];
            let signal = [render_finished];
            let submit = [vk::SubmitInfo::default()
                .wait_semaphores(&wait)
                .wait_dst_stage_mask(&stages)
                .command_buffers(&cmds)
                .signal_semaphores(&signal)];
            gfx.device.queue_submit(gfx.queue, &submit, frame.fence)?;
            let swapchains = [self.swapchain];
            let indices = [info.image_index];
            let present =
                vk::PresentInfoKHR::default().wait_semaphores(&signal).swapchains(&swapchains).image_indices(&indices);
            match gfx.swapchain_fn.as_ref().unwrap().queue_present(gfx.queue, &present) {
                Ok(suboptimal) => {
                    if suboptimal {
                        self.dirty = true;
                    }
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.dirty = true,
                Err(e) => return Err(e.into()),
            }
        }
        self.current = (self.current + 1) % FRAMES_IN_FLIGHT;
        Ok(())
    }

    pub fn destroy(&mut self, gfx: &Gfx) {
        gfx.wait_idle();
        self.destroy_views(gfx);
        unsafe {
            for f in self.frames.drain(..) {
                gfx.device.destroy_fence(f.fence, None);
                gfx.device.destroy_semaphore(f.image_available, None);
            }
            gfx.device.destroy_command_pool(self.pool, None);
            if let Some(sc) = &gfx.swapchain_fn {
                sc.destroy_swapchain(self.swapchain, None);
            }
            if let Some(s) = &gfx.surface_fn {
                s.destroy_surface(self.surface, None);
            }
        }
    }
}
