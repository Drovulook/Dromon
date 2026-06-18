use crate::app::{
    engine::{renderer::buffer::Buffer, rendering_context::RenderingContext},
    logger::Logger,
};
use anyhow::Result;
use ash::vk;
use image::ImageReader;
use std::sync::Arc;

pub struct TextureHandler {
    context: Arc<RenderingContext>,
    logger: Arc<Logger>,
    staging_buffer: Buffer,
    image_width: u32,
    image_height: u32,
    pub texture_image: vk::Image,
    pub texture_image_memory: vk::DeviceMemory,
}

impl TextureHandler {
    pub fn new(context: Arc<RenderingContext>, logger: Arc<Logger>) -> Result<Self> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/res/textures/texture.jpg");

        let img = ImageReader::open(path)?.decode()?.to_rgba8();
        let (image_width, image_height) = img.dimensions();
        let pixels: &[u8] = &img; // RgbaImage se déréférence en &[u8]

        logger.info(&format!(
            "Texture chargée : {}x{} — {} octets",
            image_width,
            image_height,
            pixels.len(),
        ));

        let image_size = pixels.len() as vk::DeviceSize;

        // staging bugger
        let staging_buffer = Buffer::new(
            context.clone(),
            image_size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        staging_buffer.map_and_unmap(pixels)?;

        let (texture_image, texture_image_memory) = context.create_image(
            image_width,
            image_height,
            vk::Format::R8G8B8A8_SRGB,
            vk::ImageTiling::OPTIMAL,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        Ok(Self {
            context,
            logger,
            image_width,
            image_height,
            staging_buffer,
            texture_image,
            texture_image_memory,
        })
    }

    pub fn copy_buffer_to_image(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        let region = vk::BufferImageCopy::default()
            .buffer_offset(0)
            .buffer_row_length(0)
            .buffer_image_height(0)
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .mip_level(0)
                    .base_array_layer(0)
                    .layer_count(1),
            )
            .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
            .image_extent(vk::Extent3D {
                width: self.image_width,
                height: self.image_height,
                depth: 1,
            });

        unsafe {
            self.context.device.cmd_copy_buffer_to_image(
                *command_buffer,
                self.staging_buffer.buffer,
                self.texture_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
        }
        Ok(())
    }
}
