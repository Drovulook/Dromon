use anyhow::{Result, ensure};
use ash::vk;
use glam::Vec4;
use std::sync::Arc;

use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::renderer::descriptors::DescriptorHandler;
use crate::app::engine::renderer::image_layout_state::ImageLayoutState;
use crate::app::engine::renderer::render_resources::texture::Texture;
use crate::app::engine::renderer::render_resources::texture_handler::TextureHandler;
use crate::app::engine::renderer::renderer_initialize::submit_one_time;
use crate::app::engine::rendering_context::RenderingContext;
use crate::app::engine::terrain_generation::MacroMap;

/// **Cartes du monde** envoyées au GPU : des grilles de la carte macro, lues par les
/// shaders (eau, plus tard terrain et ciel) via le set 3.
///
/// Statiques : construites depuis la `MacroMap`, envoyées dans la passe de transfert du
/// démarrage, puis seulement au hot reload de la carte. Seule exception : le masque des
/// chunks installés.
pub struct WorldMaps {
    context: Arc<RenderingContext>,
    /// Binding 0 : distance signée à la côte (voxels, > 0 terre), `R32_SFLOAT`.
    coast_distance: Texture,
    /// Binding 1 : climat, sur la grille climatique, `R32G32B32A32_SFLOAT`.
    /// R = température de l'air (°C), G = précipitations (normalisées), B = SST (°C),
    /// A = libre (0).
    climate: Texture,
    /// Binding 2 : chunks installés (`LoadedMask` du terrain), `R8_UNORM`, adressage
    /// torique. **Réécrit en cours de jeu**, contrairement aux autres.
    loaded_chunks: Texture,
    /// Stagings de l'envoi initial, dans l'ordre des bindings ; vidé par
    /// `release_texture_staging`.
    startup_staging: Vec<Buffer>,
    /// Staging du masque, en anneau : chacun est réutilisé `frames_in_flight + 1` envois
    /// plus tard (un envoi par frame au plus), donc après la fin de toute frame qui a pu
    /// le lire.
    loaded_staging: Vec<Buffer>,
    next_staging: usize,
    /// Clamp (pas de retour de l'autre côté du monde), linéaire, sans anisotropie.
    sampler: vk::Sampler,
    pub descriptor_set: vk::DescriptorSet,
    /// Passage monde → UV, pour l'UBO : `uv = (xy − x) · y` pour la grille fine
    /// (`coast_distance`), `uv = (xy − z) · w` pour la grille climatique.
    pub uv_transform: Vec4,
}

impl WorldMaps {
    /// Cartes de `map`, envoyées par la passe de transfert du démarrage (cf.
    /// [`Self::record_upload`]).
    pub fn new(
        context: Arc<RenderingContext>,
        texture_handler: &TextureHandler,
        descriptor_handler: &DescriptorHandler,
        map: &MacroMap,
        loaded_mask_size: u32,
        frames_in_flight: u64,
    ) -> Result<WorldMaps> {
        let descriptor_set = descriptor_handler.allocate_world_maps_set()?;
        let maps = Self::create(
            context,
            texture_handler,
            map,
            loaded_mask_size,
            frames_in_flight,
            descriptor_set,
        )?;
        maps.write_descriptor_set(descriptor_handler);
        Ok(maps)
    }

    /// Hot reload : nouvelles cartes depuis `map`, envoyées aussitôt. Le descriptor set
    /// est gardé et réécrit (le pool ne libère pas ses sets). Aucune frame en vol ne doit
    /// lire les cartes : à l'appelant d'attendre le GPU. En cas d'erreur, les anciennes
    /// cartes restent en place, intactes.
    pub fn hot_reload(
        &mut self,
        texture_handler: &TextureHandler,
        descriptor_handler: &DescriptorHandler,
        map: &MacroMap,
        loaded_mask_size: u32,
    ) -> Result<()> {
        // L'anneau compte `frames_in_flight + 1` stagings (cf. `new`).
        let frames_in_flight = self.loaded_staging.len() as u64 - 1;
        let mut fresh = Self::create(
            self.context.clone(),
            texture_handler,
            map,
            loaded_mask_size,
            frames_in_flight,
            self.descriptor_set,
        )?;
        submit_one_time(&self.context, |command_buffer| {
            fresh.record_upload(texture_handler, &command_buffer)
        })?;
        fresh.release_texture_staging();
        fresh.write_descriptor_set(descriptor_handler);
        // Les anciennes textures et leur sampler sont détruits ici (Drop) ; le set ne
        // les référence plus.
        *self = fresh;
        Ok(())
    }

    /// Textures et stagings des cartes, sans écrire `descriptor_set`.
    fn create(
        context: Arc<RenderingContext>,
        texture_handler: &TextureHandler,
        map: &MacroMap,
        loaded_mask_size: u32,
        frames_in_flight: u64,
        descriptor_set: vk::DescriptorSet,
    ) -> Result<WorldMaps> {
        let shape = map.shape();
        let n = shape.n as u32;
        // Grille rangée `[j·n + i]`, i selon X : c'est déjà l'ordre ligne par ligne
        // d'une texture (u selon X, v selon Y).
        let (coast_distance, coast_staging) = texture_handler.create_map_texture(
            n,
            n,
            vk::Format::R32_SFLOAT,
            bytemuck::cast_slice(map.coast_distance.values()),
        )?;

        // Les trois champs partagent la grille climatique : entrelacés en RGBA.
        let climate_shape = map.temperature.shape();
        let texels: Vec<[f32; 4]> = map
            .temperature
            .values()
            .iter()
            .zip(map.precipitation.values())
            .zip(map.sst.values())
            .map(|((&t, &p), &sst)| [t, p, sst, 0.0])
            .collect();
        let (climate, climate_staging) = texture_handler.create_map_texture(
            climate_shape.n as u32,
            climate_shape.n as u32,
            vk::Format::R32G32B32A32_SFLOAT,
            bytemuck::cast_slice(&texels),
        )?;

        // Aucun chunk installé au démarrage.
        let mask_bytes = (loaded_mask_size * loaded_mask_size) as usize;
        let (loaded_chunks, loaded_chunks_staging) = texture_handler.create_map_texture(
            loaded_mask_size,
            loaded_mask_size,
            vk::Format::R8_UNORM,
            &vec![0u8; mask_bytes],
        )?;
        let loaded_staging = (0..=frames_in_flight)
            .map(|_| {
                Buffer::new(
                    context.clone(),
                    mask_bytes as vk::DeviceSize,
                    vk::BufferUsageFlags::TRANSFER_SRC,
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                )
            })
            .collect::<Result<Vec<_>>>()?;

        let sampler = Self::create_sampler(&context)?;

        // Valeurs au centre des cellules, comme les texels : la conversion est exacte
        // sans correction d'un demi-texel.
        let extent = shape.n as f64 * shape.cell_size;
        let climate_extent = climate_shape.n as f64 * climate_shape.cell_size;
        let uv_transform = Vec4::new(
            shape.origin as f32,
            (1.0 / extent) as f32,
            climate_shape.origin as f32,
            (1.0 / climate_extent) as f32,
        );

        Ok(WorldMaps {
            context,
            coast_distance,
            climate,
            loaded_chunks,
            startup_staging: vec![coast_staging, climate_staging, loaded_chunks_staging],
            loaded_staging,
            next_staging: 0,
            sampler,
            descriptor_set,
            uv_transform,
        })
    }

    fn create_sampler(context: &RenderingContext) -> Result<vk::Sampler> {
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .anisotropy_enable(false)
            .unnormalized_coordinates(false)
            .compare_enable(false)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .min_lod(0.0)
            .max_lod(0.0);
        Ok(unsafe { context.device.create_sampler(&sampler_info, None) }?)
    }

    /// Écrit les cartes dans leur set. Aucune frame en vol ne doit l'utiliser.
    fn write_descriptor_set(&self, descriptor_handler: &DescriptorHandler) {
        descriptor_handler.write_world_maps(
            self.descriptor_set,
            &[
                self.coast_distance.texture_image_view,
                self.climate.texture_image_view,
                self.loaded_chunks.texture_image_view,
            ],
            self.sampler,
        );
    }

    /// Côté (texels) du masque des chunks installés.
    pub fn loaded_mask_size(&self) -> u32 {
        self.loaded_chunks.image_width
    }

    /// Copie staging → device des cartes. Passe de transfert du démarrage uniquement.
    pub fn record_upload(
        &self,
        texture_handler: &TextureHandler,
        command_buffer: &vk::CommandBuffer,
    ) -> Result<()> {
        ensure!(
            !self.startup_staging.is_empty(),
            "cartes du monde déjà envoyées : staging libéré"
        );
        let textures = [&self.coast_distance, &self.climate, &self.loaded_chunks];
        for (texture, staging) in textures.into_iter().zip(&self.startup_staging) {
            texture_handler.initialize(texture, staging, command_buffer)?;
        }
        Ok(())
    }

    /// Libère les stagings de l'envoi initial. Seulement une fois la passe de transfert
    /// **exécutée** (après l'attente de la queue).
    pub fn release_texture_staging(&mut self) {
        self.startup_staging.clear();
    }

    /// Remplace le masque des chunks installés par `texels`. À enregistrer en tête de
    /// frame, **hors** de tout rendering scope, une fois par frame au plus.
    ///
    /// Les barrières portent sur tout ce qui précède dans la file, frames précédentes
    /// comprises : la copie attend qu'elles aient fini de lire le masque, et les passes
    /// de cette frame attendent la copie.
    pub fn record_loaded_chunks_upload(
        &mut self,
        command_buffer: vk::CommandBuffer,
        texels: &[u8],
    ) -> Result<()> {
        let staging = &self.loaded_staging[self.next_staging];
        self.next_staging = (self.next_staging + 1) % self.loaded_staging.len();
        staging.upload(texels)?;

        let image = self.loaded_chunks.texture_image;
        let read = ImageLayoutState {
            access_mask: vk::AccessFlags2::SHADER_SAMPLED_READ,
            layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            stage_mask: vk::PipelineStageFlags2::VERTEX_SHADER
                | vk::PipelineStageFlags2::FRAGMENT_SHADER,
            queue_family_index: vk::QUEUE_FAMILY_IGNORED,
        };
        let write = ImageLayoutState {
            access_mask: vk::AccessFlags2::TRANSFER_WRITE,
            layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            stage_mask: vk::PipelineStageFlags2::COPY,
            queue_family_index: vk::QUEUE_FAMILY_IGNORED,
        };
        self.context
            .transition_image_layout(command_buffer, &[(image, read, write)], 1);

        let region = vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D {
                width: self.loaded_chunks.image_width,
                height: self.loaded_chunks.image_height,
                depth: 1,
            });
        unsafe {
            self.context.device.cmd_copy_buffer_to_image(
                command_buffer,
                staging.buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
        }

        self.context
            .transition_image_layout(command_buffer, &[(image, write, read)], 1);
        Ok(())
    }
}

impl Drop for WorldMaps {
    fn drop(&mut self) {
        unsafe { self.context.device.destroy_sampler(self.sampler, None) };
    }
}
