use crate::app::{
    engine::{
        renderer::{
            buffer::Buffer, descriptors::DescriptorHandler, image_layout_state::ImageLayoutState,
            render_resources::texture::Texture,
        },
        rendering_context::RenderingContext,
    },
    logger::Logger,
};
use anyhow::Result;
use ash::vk;
use image::ImageReader;
use std::sync::Arc;

pub struct TextureHandler {
    pub context: Arc<RenderingContext>,
    pub logger: Arc<Logger>,
    pub texture_sampler: vk::Sampler,
    descriptor_handler: Arc<DescriptorHandler>,
}

impl TextureHandler {
    pub fn new(
        context: Arc<RenderingContext>,
        logger: Arc<Logger>,
        descriptor_handler: Arc<DescriptorHandler>,
    ) -> Result<TextureHandler> {
        let texture_sampler = Self::create_texture_sampler(&context)?;
        Ok(TextureHandler {
            context,
            logger,
            texture_sampler,
            descriptor_handler,
        })
    }

    fn create_texture_sampler(context: &RenderingContext) -> Result<vk::Sampler> {
        let properties = context.physical_device.properties;
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::REPEAT)
            .address_mode_v(vk::SamplerAddressMode::REPEAT)
            .address_mode_w(vk::SamplerAddressMode::REPEAT)
            .anisotropy_enable(true)
            .max_anisotropy(properties.limits.max_sampler_anisotropy)
            .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
            .unnormalized_coordinates(false)
            .compare_enable(false)
            .compare_op(vk::CompareOp::ALWAYS)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
            .mip_lod_bias(0.0)
            .min_lod(0.0)
            .max_lod(vk::LOD_CLAMP_NONE);

        let sampler = unsafe { context.device.create_sampler(&sampler_info, None) }?;
        Ok(sampler)
    }

    pub fn create_texture(&self, path: &str) -> Result<Texture> {
        let full_path = format!("{}{}", env!("CARGO_MANIFEST_DIR"), path);

        let img = ImageReader::open(full_path)?.decode()?.to_rgba8();
        let (image_width, image_height) = img.dimensions();
        let pixels: &[u8] = &img; // RgbaImage se déréférence en &[u8]

        self.logger.info(&format!(
            "Texture chargée : {}x{} — {} octets",
            image_width,
            image_height,
            pixels.len(),
        ));

        let mip_levels = ((image_width.max(image_height) as f32).log2().floor() as u32) + 1;

        let image_size = pixels.len() as vk::DeviceSize;

        // staging bugger
        let staging_buffer = Buffer::new(
            self.context.clone(),
            image_size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        staging_buffer.map_and_unmap(pixels)?;

        let (texture_image, texture_image_memory) = self.context.create_image(
            image_width,
            image_height,
            mip_levels,
            vk::SampleCountFlags::TYPE_1,
            vk::Format::R8G8B8A8_SRGB,
            vk::ImageTiling::OPTIMAL,
            vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::TRANSFER_SRC // for generating mipmaps
                | vk::ImageUsageFlags::SAMPLED,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        let texture_image_view = self.context.create_image_view(
            &texture_image,
            vk::Format::R8G8B8A8_SRGB,
            vk::ImageAspectFlags::COLOR,
            mip_levels,
        )?;

        // create one descriptor set (with one descriptor) per texture
        let descriptor_set = self
            .descriptor_handler
            .create_texture_descriptor_set(&texture_image_view, &self.texture_sampler)?;

        Ok(Texture::new(
            self.context.clone(),
            image_width,
            image_height,
            mip_levels,
            staging_buffer,
            texture_image,
            texture_image_memory,
            texture_image_view,
            descriptor_set,
        ))
    }

    // put the image from the staging buffer into the
    // image
    pub fn initialize(&self, texture: &Texture, command_buffer: &vk::CommandBuffer) -> Result<()> {
        // L'image est créée en UNDEFINED ; la copie exige TRANSFER_DST_OPTIMAL.
        self.context.transition_image_layout(
            *command_buffer,
            &[(
                texture.texture_image,
                ImageLayoutState::default(),
                ImageLayoutState {
                    access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                    layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    stage_mask: vk::PipelineStageFlags2::COPY,
                    queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                },
            )],
            texture.mip_levels,
        );

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
                width: texture.image_width,
                height: texture.image_height,
                depth: 1,
            });

        unsafe {
            self.context.device.cmd_copy_buffer_to_image(
                *command_buffer,
                texture.staging_buffer.buffer,
                texture.texture_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );

            self.context.generate_mipmaps(
                *command_buffer,
                &texture.texture_image,
                vk::Format::R8G8B8A8_SRGB,
                texture.image_width,
                texture.image_height,
                texture.mip_levels,
            )?;
        }
        Ok(())
    }
}

impl Drop for TextureHandler {
    fn drop(&mut self) {
        unsafe {
            self.context
                .device
                .destroy_sampler(self.texture_sampler, None);
        }
    }
}
