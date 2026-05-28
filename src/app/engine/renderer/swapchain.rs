use crate::app::engine::rendering_context::{RenderingContext, SwapchainSurface};
use anyhow::Result;
use ash::vk;
use std::sync::Arc;
use winit::window::Window;

pub struct Swapchain {
    pub desired_image_count: u32,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub image_views: Vec<vk::ImageView>,
    pub images: Vec<vk::Image>,
    handle: vk::SwapchainKHR,
    surface: SwapchainSurface,
    window: Arc<Window>,
    context: Arc<RenderingContext>,
    is_dirty: bool,
}

impl Swapchain {
    pub fn new(context: Arc<RenderingContext>, window: Arc<Window>) -> Result<Self> {
        let surface = unsafe { context.create_surface(window.clone())? };
        let format = vk::Format::B8G8R8A8_SRGB;
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
            format,
            extent,
            image_views: Default::default(),
            images: Default::default(),
            handle: Default::default(),
            surface,
            window,
            context,
            is_dirty: true,
        })
    }

    pub fn update_size(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        self.extent = vk::Extent2D {
            width: size.width,
            height: size.height,
        };

        if self.extent.width == 0 || self.extent.height == 0 {
            return Ok(());
        }

        unsafe {
            let new_swapchain = self.context.swapchain_extensions.create_swapchain(
                &vk::SwapchainCreateInfoKHR::default()
                    .surface(self.surface.handle)
                    .min_image_count(self.desired_image_count)
                    .image_format(self.format)
                    .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
                    .image_extent(self.extent)
                    .image_array_layers(1)
                    .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                    .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .pre_transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
                    .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                    .present_mode(vk::PresentModeKHR::FIFO)
                    .clipped(true)
                    .old_swapchain(self.handle),
                None,
            )?;

            for image_view in self.image_views.drain(..) {
                self.context.device.destroy_image_view(image_view, None);
            }
            self.images.clear();
            self.context
                .swapchain_extensions
                .destroy_swapchain(self.handle, None);

            self.handle = new_swapchain;
            self.images = self
                .context
                .swapchain_extensions
                .get_swapchain_images(self.handle)?;
            for image in &self.images {
                self.image_views.push(self.context.create_image_view(
                    *image,
                    self.format,
                    vk::ImageAspectFlags::COLOR,
                )?);
            }
        }
        Ok(())
    }

    pub fn aquire_next_image(&mut self, image_available_semaphore: vk::Semaphore) -> Result<u32> {
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
            self.context.swapchain_extensions.queue_present(
                self.context.queues[self.context.queue_families.present as usize],
                &vk::PresentInfoKHR::default()
                    .wait_semaphores(&[render_finished_semaphore])
                    .swapchains(&[self.handle])
                    .image_indices(&[image_index]),
            )
        }?;

        if is_suboptimal {
            self.is_dirty = true;
        }

        Ok(())
    }
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        unsafe {
            for image_view in self.image_views.drain(..) {
                self.context.device.destroy_image_view(image_view, None);
            }
            self.context
                .swapchain_extensions
                .destroy_swapchain(self.handle, None);

            self.context
                .surface_extensions
                .destroy_surface(self.surface.handle, None);
        }
    }
}
