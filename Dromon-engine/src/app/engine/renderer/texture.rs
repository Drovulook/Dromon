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
    pub texture_image: vk::Image,
    pub texture_image_memory: vk::DeviceMemory,
}

impl TextureHandler {
    pub fn new(context: Arc<RenderingContext>, logger: Arc<Logger>) -> Result<Self> {
        Self::create_texture_image(context.clone(), &logger);
        Ok(Self { context, logger })
    }

    fn create_texture_image(context: Arc<RenderingContext>, logger: &Logger) -> Result<()> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/res/textures/texture.jpg");

        let img = ImageReader::open(path)?.decode()?.to_rgba8();
        let (width, height) = img.dimensions();
        let pixels: &[u8] = &img; // RgbaImage se déréférence en &[u8]

        logger.info(&format!(
            "Texture chargée : {}x{} — {} octets",
            width,
            height,
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

        let texture_image = unsafe {
            context.device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_SRGB)
                    .extent(vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?
        };

        Ok(())
    }
}
