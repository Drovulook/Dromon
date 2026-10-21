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
    /// Sampler des textures de surface (objets, terrain) : répétition, trilinéaire,
    /// anisotropie max.
    pub base_texture_sampler: vk::Sampler,
    descriptor_handler: Arc<DescriptorHandler>,
}

impl TextureHandler {
    pub fn new(
        context: Arc<RenderingContext>,
        logger: Arc<Logger>,
        descriptor_handler: Arc<DescriptorHandler>,
    ) -> Result<TextureHandler> {
        let base_texture_sampler = Self::create_base_texture_sampler(&context)?;
        Ok(TextureHandler {
            context,
            logger,
            base_texture_sampler,
            descriptor_handler,
        })
    }

    fn create_base_texture_sampler(context: &RenderingContext) -> Result<vk::Sampler> {
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

    /// Texture d'objet : image couleur lue depuis un fichier, en sRGB, avec mipmaps et
    /// son propre descriptor set (set 1). Rend aussi son staging (cf.
    /// `create_texture_from_bytes`).
    pub fn create_object_texture(&self, path: &str) -> Result<(Texture, Buffer)> {
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

        let (mut texture, staging) = self.create_texture_from_bytes(
            image_width,
            image_height,
            vk::Format::R8G8B8A8_SRGB,
            full_mip_chain(image_width, image_height),
            1,
            pixels,
        )?;

        // create one descriptor set (with one descriptor) per texture
        texture.descriptor_set = Some(self.descriptor_handler.create_texture_descriptor_set(
            &texture.texture_image_view,
            &self.base_texture_sampler,
        )?);
        Ok((texture, staging))
    }

    /// Texture de **données** (carte du monde) : `bytes` contient les texels rangés
    /// ligne par ligne au `format` donné (flottant ou UNORM, jamais sRGB : la
    /// correction gamma fausserait les valeurs). Sans mipmaps ni descriptor set : les
    /// cartes sont lues avec `SampleLevel(…, 0)` et regroupées dans un set commun.
    pub fn create_map_texture(
        &self,
        width: u32,
        height: u32,
        format: vk::Format,
        bytes: &[u8],
    ) -> Result<(Texture, Buffer)> {
        self.create_texture_from_bytes(width, height, format, 1, 1, bytes)
    }

    /// **Texture array** de `array_layers` couches `width × height`, avec mipmaps.
    /// `bytes` = les couches bout à bout, chacune rangée ligne par ligne au `format`.
    /// Sans descriptor set : l'appelant l'intègre au sien.
    pub fn create_texture_array(
        &self,
        width: u32,
        height: u32,
        array_layers: u32,
        format: vk::Format,
        bytes: &[u8],
    ) -> Result<(Texture, Buffer)> {
        let mip_levels = full_mip_chain(width, height);
        self.create_texture_from_bytes(width, height, format, mip_levels, array_layers, bytes)
    }

    /// Image device-local et sa vue + staging rempli ; la copie de l'un vers l'autre
    /// reste à enregistrer (`initialize`). Le staging est rendu à part : l'appelant le
    /// garde jusqu'à la **fin d'exécution** de la copie, puis le libère.
    fn create_texture_from_bytes(
        &self,
        width: u32,
        height: u32,
        format: vk::Format,
        mip_levels: u32,
        array_layers: u32,
        bytes: &[u8],
    ) -> Result<(Texture, Buffer)> {
        // staging buffer
        let staging_buffer = Buffer::new(
            self.context.clone(),
            bytes.len() as vk::DeviceSize,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        staging_buffer.upload(bytes)?;

        let (texture_image, texture_image_memory) = self.context.create_image_layers(
            width,
            height,
            mip_levels,
            array_layers,
            vk::SampleCountFlags::TYPE_1,
            format,
            vk::ImageTiling::OPTIMAL,
            vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::TRANSFER_SRC // for generating mipmaps
                | vk::ImageUsageFlags::SAMPLED,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        let texture_image_view = self.context.create_image_view_layers(
            &texture_image,
            format,
            vk::ImageAspectFlags::COLOR,
            mip_levels,
            array_layers,
        )?;

        let texture = Texture::new(
            self.context.clone(),
            width,
            height,
            mip_levels,
            array_layers,
            format,
            texture_image,
            texture_image_memory,
            texture_image_view,
            None,
        );
        Ok((texture, staging_buffer))
    }

    /// Enregistre la copie `staging` → `texture` (+ mipmaps et passage en lecture).
    /// Seulement **enregistrée** : `staging` doit vivre jusqu'à la fin d'exécution du
    /// command buffer.
    pub fn initialize(
        &self,
        texture: &Texture,
        staging: &Buffer,
        command_buffer: &vk::CommandBuffer,
    ) -> Result<()> {
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
                    .layer_count(texture.array_layers),
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
                staging.buffer,
                texture.texture_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );

            // Avec mip_levels = 1, ne fait que la transition finale vers
            // SHADER_READ_ONLY_OPTIMAL.
            self.context.generate_mipmaps(
                *command_buffer,
                &texture.texture_image,
                texture.format,
                texture.image_width,
                texture.image_height,
                texture.mip_levels,
                texture.array_layers,
            )?;
        }
        Ok(())
    }
}

/// Nombre de niveaux d'une chaîne de mipmaps complète (jusqu'à 1 × 1).
fn full_mip_chain(width: u32, height: u32) -> u32 {
    width.max(height).ilog2() + 1
}

impl Drop for TextureHandler {
    fn drop(&mut self) {
        unsafe {
            self.context
                .device
                .destroy_sampler(self.base_texture_sampler, None);
        }
    }
}
