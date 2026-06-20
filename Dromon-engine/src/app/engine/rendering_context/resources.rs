use super::RenderingContext;
use crate::app::engine::rendering_context::{ImageLayoutState, SwapchainSurface};
use anyhow::Result;
use ash::vk;
use std::sync::Arc;
use winit::{
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::Window,
};

impl RenderingContext {
    /// # Safety
    /// The window should outlive the surface
    pub unsafe fn create_surface(&self, window: Arc<Window>) -> Result<SwapchainSurface> {
        let raw_display_handle = window.display_handle()?.as_raw();
        let raw_window_handle = window.window_handle()?.as_raw();
        unsafe {
            let handle = ash_window::create_surface(
                &self.entry,
                &self.instance,
                raw_display_handle,
                raw_window_handle,
                None,
            )?;
            let capabilities = self
                .surface_extensions
                .get_physical_device_surface_capabilities(self.physical_device.handle, handle)?;

            let formats = self
                .surface_extensions
                .get_physical_device_surface_formats(self.physical_device.handle, handle)?;

            let present_modes = self
                .surface_extensions
                .get_physical_device_surface_present_modes(self.physical_device.handle, handle)?;

            Ok(SwapchainSurface {
                handle,
                capabilities,
                formats,
                present_modes,
            })
        }
    }

    pub fn create_image_view(
        &self,
        image: &vk::Image,
        format: vk::Format,
        aspect_flags: vk::ImageAspectFlags,
    ) -> Result<vk::ImageView> {
        let image_view = unsafe {
            self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(*image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .components(vk::ComponentMapping::default())
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(aspect_flags)
                            .base_mip_level(0)
                            .level_count(1)
                            .base_array_layer(0)
                            .layer_count(1),
                    ),
                None,
            )
        }?;
        Ok(image_view)
    }

    pub fn create_image(
        &self,
        width: u32,
        height: u32,
        format: vk::Format,
        tiling: vk::ImageTiling,
        usage: vk::ImageUsageFlags,
        memory_properties: vk::MemoryPropertyFlags,
    ) -> Result<(vk::Image, vk::DeviceMemory)> {
        let image = unsafe {
            self.device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(tiling)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?
        };

        let mem_requirements = unsafe { self.device.get_image_memory_requirements(image) };

        let image_memory = unsafe {
            self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(mem_requirements.size)
                    .memory_type_index(self.find_memory_type(
                        mem_requirements.memory_type_bits, // ← le masque, pas image_size
                        memory_properties,
                    )?),
                None,
            )?
        };
        unsafe {
            self.device.bind_image_memory(image, image_memory, 0)?;
        }

        Ok((image, image_memory))
    }

    pub fn find_memory_type(
        &self,
        type_filter: u32,
        properties: vk::MemoryPropertyFlags,
    ) -> Result<u32> {
        let memory_properties = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.physical_device.handle)
        };
        (0..memory_properties.memory_type_count)
            .find(|&i| {
                (type_filter & (1 << i) != 0)
                    && (memory_properties.memory_types[i as usize].property_flags & properties)
                        == properties
            })
            .ok_or_else(|| anyhow::anyhow!("Aucun type mémoire compatible trouvé"))
    }
}
