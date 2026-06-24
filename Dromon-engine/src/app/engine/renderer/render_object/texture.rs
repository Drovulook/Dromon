use std::sync::Arc;

use ash::vk;

use crate::app::engine::{renderer::buffer::Buffer, rendering_context::RenderingContext};

pub struct Texture {
    context: Arc<RenderingContext>,
    pub image_width: u32,
    pub image_height: u32,
    pub mip_levels: u32,
    pub staging_buffer: Buffer,
    pub texture_image: vk::Image,
    pub texture_image_memory: vk::DeviceMemory,
    pub texture_image_view: vk::ImageView,
    pub descriptor_set: vk::DescriptorSet,
}

impl Texture {
    pub fn new(
        context: Arc<RenderingContext>,
        image_width: u32,
        image_height: u32,
        mip_levels: u32,
        staging_buffer: Buffer,
        texture_image: vk::Image,
        texture_image_memory: vk::DeviceMemory,
        texture_image_view: vk::ImageView,
        descriptor_set: vk::DescriptorSet,
    ) -> Texture {
        Texture {
            context,
            image_width,
            image_height,
            mip_levels,
            staging_buffer,
            texture_image,
            texture_image_memory,
            texture_image_view,
            descriptor_set,
        }
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe {
            self.context
                .device
                .destroy_image_view(self.texture_image_view, None);
            self.context.device.destroy_image(self.texture_image, None);
            self.context
                .device
                .free_memory(self.texture_image_memory, None);
        }
    }
}
