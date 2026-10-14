use anyhow::Result;
use ash::vk;
use glam::Vec4;
use std::sync::Arc;

use crate::app::engine::renderer::descriptors::DescriptorHandler;
use crate::app::engine::renderer::render_resources::texture::Texture;
use crate::app::engine::renderer::render_resources::texture_handler::TextureHandler;
use crate::app::engine::rendering_context::RenderingContext;
use crate::app::engine::terrain_generation::MacroMap;

/// **Cartes du monde** envoyées au GPU : des grilles de la carte macro, lues par les
/// shaders (eau, plus tard terrain et ciel) via le set 3.
///
/// Statiques : construites une fois depuis la `MacroMap`, envoyées dans la passe de
/// transfert du démarrage.
pub struct WorldMaps {
    context: Arc<RenderingContext>,
    /// Binding 0 : distance signée à la côte (voxels, > 0 terre), `R32_SFLOAT`.
    coast_distance: Texture,
    /// Clamp (pas de retour de l'autre côté du monde), linéaire, sans anisotropie.
    sampler: vk::Sampler,
    pub descriptor_set: vk::DescriptorSet,
    /// Passage monde → UV, pour l'UBO : `uv = (xy − x) · y`.
    pub uv_transform: Vec4,
}

impl WorldMaps {
    pub fn new(
        context: Arc<RenderingContext>,
        texture_handler: &TextureHandler,
        descriptor_handler: &DescriptorHandler,
        map: &MacroMap,
    ) -> Result<WorldMaps> {
        let shape = map.shape();
        let n = shape.n as u32;
        // Grille rangée `[j·n + i]`, i selon X : c'est déjà l'ordre ligne par ligne
        // d'une texture (u selon X, v selon Y).
        let coast_distance = texture_handler.create_map_texture(
            n,
            n,
            vk::Format::R32_SFLOAT,
            bytemuck::cast_slice(map.coast_distance.values()),
        )?;

        let sampler = Self::create_sampler(&context)?;
        let descriptor_set = descriptor_handler
            .create_world_maps_descriptor_set(&[coast_distance.texture_image_view], sampler)?;

        // Valeurs au centre des cellules, comme les texels : la conversion est exacte
        // sans correction d'un demi-texel.
        let extent = shape.n as f64 * shape.cell_size;
        let uv_transform = Vec4::new(shape.origin as f32, (1.0 / extent) as f32, 0.0, 0.0);

        Ok(WorldMaps {
            context,
            coast_distance,
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

    /// Copie staging → device des cartes. Passe de transfert du démarrage uniquement.
    pub fn record_upload(
        &self,
        texture_handler: &TextureHandler,
        command_buffer: &vk::CommandBuffer,
    ) -> Result<()> {
        texture_handler.initialize(&self.coast_distance, command_buffer)
    }
}

impl Drop for WorldMaps {
    fn drop(&mut self) {
        unsafe { self.context.device.destroy_sampler(self.sampler, None) };
    }
}
