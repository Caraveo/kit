//! GPU buffers and images.

use crate::{Gfx, vk};
use anyhow::{Context, Result};
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme};

/// A Vulkan buffer with its memory. Destroy explicitly with [`Buffer::destroy`].
pub struct Buffer {
    pub buffer: vk::Buffer,
    allocation: Option<Allocation>,
    pub size: u64,
}

impl Buffer {
    pub fn new(
        gfx: &Gfx,
        size: u64,
        usage: vk::BufferUsageFlags,
        location: MemoryLocation,
        name: &str,
    ) -> Result<Self> {
        let size = size.max(4);
        let device = &gfx.device;
        let buffer = unsafe {
            device.create_buffer(
                &vk::BufferCreateInfo::default().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?
        };
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let allocation = gfx
            .allocator()
            .lock()
            .unwrap()
            .allocate(&AllocationCreateDesc {
                name,
                requirements,
                location,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .with_context(|| format!("allocating buffer {name}"))?;
        unsafe { device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset())? };
        Ok(Self { buffer, allocation: Some(allocation), size })
    }

    /// Write into a host-visible buffer.
    pub fn write<T: bytemuck::Pod>(&mut self, offset: usize, data: &[T]) {
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let slice = self.allocation.as_mut().and_then(|a| a.mapped_slice_mut()).expect("buffer is not host visible");
        slice[offset..offset + bytes.len()].copy_from_slice(bytes);
    }

    /// Contents of a host-visible buffer.
    pub fn mapped(&self) -> Option<&[u8]> {
        self.allocation.as_ref().and_then(|a| a.mapped_slice())
    }

    /// Create a device-local buffer filled with `data` (uploaded through a staging buffer).
    pub fn with_data<T: bytemuck::Pod>(gfx: &Gfx, usage: vk::BufferUsageFlags, data: &[T], name: &str) -> Result<Self> {
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let mut staging = Buffer::new(
            gfx,
            bytes.len() as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
            "staging",
        )?;
        staging.write(0, bytes);
        let buf = Buffer::new(
            gfx,
            bytes.len() as u64,
            usage | vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuOnly,
            name,
        )?;
        let result = gfx.one_shot(|cmd| unsafe {
            gfx.device.cmd_copy_buffer(
                cmd,
                staging.buffer,
                buf.buffer,
                &[vk::BufferCopy::default().size(bytes.len() as u64)],
            );
        });
        staging.destroy(gfx);
        result?;
        Ok(buf)
    }

    pub fn destroy(&mut self, gfx: &Gfx) {
        if let Some(a) = self.allocation.take() {
            let _ = gfx.allocator().lock().unwrap().free(a);
        }
        if self.buffer != vk::Buffer::null() {
            unsafe { gfx.device.destroy_buffer(self.buffer, None) };
            self.buffer = vk::Buffer::null();
        }
    }
}

/// A 2D Vulkan image with a view. Destroy explicitly with [`Image::destroy`].
pub struct Image {
    pub image: vk::Image,
    pub view: vk::ImageView,
    allocation: Option<Allocation>,
    pub extent: vk::Extent2D,
    pub format: vk::Format,
    pub aspect: vk::ImageAspectFlags,
    pub samples: vk::SampleCountFlags,
}

impl Image {
    pub fn new(
        gfx: &Gfx,
        extent: vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        samples: vk::SampleCountFlags,
        name: &str,
    ) -> Result<Self> {
        let aspect = if format == crate::DEPTH_FORMAT || format == vk::Format::D24_UNORM_S8_UINT {
            vk::ImageAspectFlags::DEPTH
        } else {
            vk::ImageAspectFlags::COLOR
        };
        let device = &gfx.device;
        let image = unsafe {
            device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D { width: extent.width.max(1), height: extent.height.max(1), depth: 1 })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(samples)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )?
        };
        let requirements = unsafe { device.get_image_memory_requirements(image) };
        let allocation = gfx
            .allocator()
            .lock()
            .unwrap()
            .allocate(&AllocationCreateDesc {
                name,
                requirements,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .with_context(|| format!("allocating image {name}"))?;
        unsafe { device.bind_image_memory(image, allocation.memory(), allocation.offset())? };
        let view = unsafe {
            device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(
                        vk::ImageSubresourceRange::default().aspect_mask(aspect).level_count(1).layer_count(1),
                    ),
                None,
            )?
        };
        Ok(Self { image, view, allocation: Some(allocation), extent, format, aspect, samples })
    }

    /// Upload an RGBA8 texture. The image ends in `SHADER_READ_ONLY_OPTIMAL`.
    pub fn texture_rgba8(gfx: &Gfx, width: u32, height: u32, rgba: &[u8], srgb: bool, name: &str) -> Result<Self> {
        assert_eq!(rgba.len(), (width * height * 4) as usize, "texture data size mismatch");
        let format = if srgb { vk::Format::R8G8B8A8_SRGB } else { vk::Format::R8G8B8A8_UNORM };
        let extent = vk::Extent2D { width, height };
        let img = Image::new(
            gfx,
            extent,
            format,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::SampleCountFlags::TYPE_1,
            name,
        )?;
        let mut staging = Buffer::new(
            gfx,
            rgba.len() as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
            "texture staging",
        )?;
        staging.write(0, rgba);
        let result = gfx.one_shot(|cmd| {
            gfx.image_barrier(
                cmd,
                img.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                (vk::PipelineStageFlags::TOP_OF_PIPE, vk::AccessFlags::empty()),
                (vk::PipelineStageFlags::TRANSFER, vk::AccessFlags::TRANSFER_WRITE),
            );
            let region = vk::BufferImageCopy::default()
                .image_subresource(
                    vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1),
                )
                .image_extent(vk::Extent3D { width, height, depth: 1 });
            unsafe {
                gfx.device.cmd_copy_buffer_to_image(
                    cmd,
                    staging.buffer,
                    img.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[region],
                );
            }
            gfx.image_barrier(
                cmd,
                img.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                (vk::PipelineStageFlags::TRANSFER, vk::AccessFlags::TRANSFER_WRITE),
                (vk::PipelineStageFlags::FRAGMENT_SHADER, vk::AccessFlags::SHADER_READ),
            );
        });
        staging.destroy(gfx);
        result?;
        Ok(img)
    }

    /// Copy a color image (in `TRANSFER_SRC_OPTIMAL`) back to the CPU as RGBA8.
    /// Used for screenshots and tests. Assumes a 4-byte-per-pixel format.
    pub fn read_back(&self, gfx: &Gfx, layout: vk::ImageLayout) -> Result<Vec<u8>> {
        let size = (self.extent.width * self.extent.height * 4) as u64;
        let buf = Buffer::new(gfx, size, vk::BufferUsageFlags::TRANSFER_DST, MemoryLocation::GpuToCpu, "readback")?;
        let result = gfx.one_shot(|cmd| {
            gfx.image_barrier(
                cmd,
                self.image,
                vk::ImageAspectFlags::COLOR,
                layout,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                (vk::PipelineStageFlags::ALL_COMMANDS, vk::AccessFlags::MEMORY_WRITE),
                (vk::PipelineStageFlags::TRANSFER, vk::AccessFlags::TRANSFER_READ),
            );
            let region = vk::BufferImageCopy::default()
                .image_subresource(
                    vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1),
                )
                .image_extent(vk::Extent3D { width: self.extent.width, height: self.extent.height, depth: 1 });
            unsafe {
                gfx.device.cmd_copy_image_to_buffer(
                    cmd,
                    self.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buf.buffer,
                    &[region],
                );
            }
            gfx.image_barrier(
                cmd,
                self.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                layout,
                (vk::PipelineStageFlags::TRANSFER, vk::AccessFlags::TRANSFER_READ),
                (vk::PipelineStageFlags::ALL_COMMANDS, vk::AccessFlags::MEMORY_READ),
            );
        });
        let mut buf = buf;
        let data = buf
            .allocation
            .as_ref()
            .and_then(|a| a.mapped_slice())
            .map(|s| s[..size as usize].to_vec())
            .context("readback buffer not mapped");
        buf.destroy(gfx);
        result?;
        data
    }

    pub fn destroy(&mut self, gfx: &Gfx) {
        unsafe {
            if self.view != vk::ImageView::null() {
                gfx.device.destroy_image_view(self.view, None);
                self.view = vk::ImageView::null();
            }
            if let Some(a) = self.allocation.take() {
                let _ = gfx.allocator().lock().unwrap().free(a);
            }
            if self.image != vk::Image::null() {
                gfx.device.destroy_image(self.image, None);
                self.image = vk::Image::null();
            }
        }
    }
}
