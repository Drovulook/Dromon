use crate::app::engine::rendering_context::{RenderingContext, SwapchainSurface};
use crate::app::logger::Logger;
use anyhow::{Result, anyhow};
use ash::vk;
use std::sync::Arc;
use winit::window::Window;

pub struct Swapchain {
    pub desired_image_count: u32,
    pub extent: vk::Extent2D,
    // color images
    pub color_format: vk::Format,
    pub color_image_views: Vec<vk::ImageView>,
    pub color_images: Vec<vk::Image>,
    // depth image
    pub depth_format: vk::Format,
    pub depth_image: vk::Image,
    depth_image_memory: vk::DeviceMemory,
    pub depth_image_view: vk::ImageView,
    //
    handle: vk::SwapchainKHR,
    surface: SwapchainSurface,
    window: Arc<Window>,
    context: Arc<RenderingContext>,
    logger: Arc<Logger>,
    pub is_dirty: bool,
}

impl Swapchain {
    pub fn new(
        context: Arc<RenderingContext>,
        window: Arc<Window>,
        logger: Arc<Logger>,
    ) -> Result<Self> {
        let surface = unsafe { context.create_surface(window.clone())? };
        let color_format = vk::Format::B8G8R8A8_SRGB;
        let depth_format = Self::find_supported_format(
            &context,
            &[
                vk::Format::D32_SFLOAT,
                vk::Format::D32_SFLOAT_S8_UINT,
                vk::Format::D24_UNORM_S8_UINT,
            ],
            vk::ImageTiling::OPTIMAL,
            vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT,
        )?;
        let extent = if surface.capabilities.current_extent.width != u32::MAX {
            surface.capabilities.current_extent
        } else {
            let size = window.inner_size();
            vk::Extent2D {
                width: size.width,
                height: size.height,
            }
        };
        let desired_image_count = (surface.capabilities.min_image_count + 1).clamp(
            surface.capabilities.min_image_count,
            if surface.capabilities.max_image_count == 0 {
                u32::MAX
            } else {
                surface.capabilities.max_image_count
            },
        );

        Ok(Self {
            desired_image_count,
            extent,
            color_format,
            color_image_views: Default::default(),
            color_images: Default::default(),
            depth_format,
            depth_image: Default::default(),
            depth_image_memory: Default::default(),
            depth_image_view: Default::default(),
            handle: Default::default(),
            surface,
            window,
            context,
            logger,
            is_dirty: true,
        })
    }

    pub fn update_size(&mut self) -> Result<()> {
        self.logger.info("update_size");
        let size = self.window.inner_size();
        self.extent = vk::Extent2D {
            width: size.width,
            height: size.height,
        };

        if self.extent.width == 0 || self.extent.height == 0 {
            return Ok(());
        }

        unsafe {
            self.context.device.device_wait_idle()?;

            let new_swapchain = self.context.swapchain_extensions.create_swapchain(
                &vk::SwapchainCreateInfoKHR::default()
                    .surface(self.surface.handle)
                    .min_image_count(self.desired_image_count)
                    .image_format(self.color_format)
                    .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
                    .image_extent(self.extent)
                    .image_array_layers(1)
                    .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                    .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .pre_transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
                    .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                    .present_mode(vk::PresentModeKHR::MAILBOX)
                    .clipped(true)
                    .old_swapchain(self.handle),
                None,
            )?;

            // color images + swapchain
            for image_view in self.color_image_views.drain(..) {
                self.context.device.destroy_image_view(image_view, None);
            }
            self.color_images.clear();
            self.context
                .swapchain_extensions
                .destroy_swapchain(self.handle, None);
            self.handle = new_swapchain;
            self.color_images = self
                .context
                .swapchain_extensions
                .get_swapchain_images(self.handle)?;
            for image in &self.color_images {
                self.color_image_views.push(self.context.create_image_view(
                    &image,
                    self.color_format,
                    vk::ImageAspectFlags::COLOR,
                    1,
                )?);
            }

            // depth image
            self.context
                .device
                .destroy_image_view(self.depth_image_view, None);
            self.context.device.destroy_image(self.depth_image, None);
            self.context
                .device
                .free_memory(self.depth_image_memory, None);

            let (depth_image, depth_image_memory) = self.context.create_image(
                self.extent.width,
                self.extent.height,
                1,
                self.depth_format,
                vk::ImageTiling::OPTIMAL,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )?;
            let depth_image_view = self.context.create_image_view(
                &depth_image,
                self.depth_format,
                vk::ImageAspectFlags::DEPTH,
                1,
            )?;
            self.depth_image = depth_image;
            self.depth_image_memory = depth_image_memory;
            self.depth_image_view = depth_image_view;
        }
        self.is_dirty = false;
        Ok(())
    }

    fn find_supported_format(
        context: &RenderingContext,
        candidates: &[vk::Format],
        tiling: vk::ImageTiling,
        features: vk::FormatFeatureFlags,
    ) -> Result<vk::Format> {
        for &format in candidates {
            let props = unsafe {
                context
                    .instance
                    .get_physical_device_format_properties(context.physical_device.handle, format)
            };

            let supported = match tiling {
                vk::ImageTiling::LINEAR => props.linear_tiling_features,
                vk::ImageTiling::OPTIMAL => props.optimal_tiling_features,
                _ => vk::FormatFeatureFlags::empty(),
            };

            if supported.contains(features) {
                return Ok(format);
            }
        }

        Err(anyhow!("Unsupported format"))
    }

    pub fn acquire_next_image(&mut self, image_available_semaphore: vk::Semaphore) -> Result<u32> {
        unsafe {
            let (image_index, is_suboptimal) =
                self.context.swapchain_extensions.acquire_next_image(
                    self.handle,
                    u64::MAX,
                    image_available_semaphore,
                    vk::Fence::null(),
                )?;
            if is_suboptimal {
                self.is_dirty = true;
                self.logger.warn("dirty");
            }
            Ok(image_index)
        }
    }

    pub fn present(
        &mut self,
        image_index: u32,
        render_finished_semaphore: vk::Semaphore,
    ) -> Result<()> {
        let is_suboptimal = unsafe {
            match self.context.swapchain_extensions.queue_present(
                self.context.queues[self.context.queue_families.present as usize],
                &vk::PresentInfoKHR::default()
                    .wait_semaphores(&[render_finished_semaphore])
                    .swapchains(&[self.handle])
                    .image_indices(&[image_index]),
            ) {
                Ok(is_suboptimal) => is_suboptimal,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => true,
                Err(error) => return Err(error.into()),
            }
        };

        if is_suboptimal {
            self.is_dirty = true;
        }

        Ok(())
    }
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        unsafe {
            let _ = self.context.device.device_wait_idle();
            // color images
            for image_view in self.color_image_views.drain(..) {
                self.context.device.destroy_image_view(image_view, None);
            }
            // depth image
            self.context
                .device
                .destroy_image_view(self.depth_image_view, None);
            self.context.device.destroy_image(self.depth_image, None);
            self.context
                .device
                .free_memory(self.depth_image_memory, None);

            self.context
                .swapchain_extensions
                .destroy_swapchain(self.handle, None);

            self.context
                .surface_extensions
                .destroy_surface(self.surface.handle, None);
        }
    }
}
