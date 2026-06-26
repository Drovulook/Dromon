use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

/// Résolution (carrée) de la shadow map, en texels. Indépendante de la fenêtre :
/// plus c'est grand, plus les ombres sont fines, mais plus ça coûte en mémoire
pub const SHADOW_MAP_RESOLUTION: u32 = 2048;

/// Format de la shadow map : profondeur 32 bits flottante, sans stencil.
const SHADOW_MAP_FORMAT: vk::Format = vk::Format::D32_SFLOAT;

/// Ressources GPU de la passe d'ombre : une image de profondeur dans laquelle on
/// rend la scène depuis le point de vue de la lumière, plus le sampler (en mode
/// comparaison) qui permet de la relire dans le shader principal.
pub struct ShadowMap {
    context: Arc<RenderingContext>,
    pub extent: vk::Extent2D,
    pub format: vk::Format,
    pub image: vk::Image,
    image_memory: vk::DeviceMemory,
    pub image_view: vk::ImageView,
    pub sampler: vk::Sampler,
}

impl ShadowMap {
    pub fn new(context: Arc<RenderingContext>) -> Result<Self> {
        let extent = vk::Extent2D {
            width: SHADOW_MAP_RESOLUTION,
            height: SHADOW_MAP_RESOLUTION,
        };

        // DEPTH_STENCIL_ATTACHMENT : cible de rendu de la passe d'ombre.
        // SAMPLED                  : on la relit ensuite dans le frag shader principal.
        // Pas de MSAA (TYPE_1) : inutile pour une carte de profondeur, et ça
        // simplifie (une image multisample n'est pas directement samplable).
        let (image, image_memory) = context.create_image(
            extent.width,
            extent.height,
            1,
            vk::SampleCountFlags::TYPE_1,
            SHADOW_MAP_FORMAT,
            vk::ImageTiling::OPTIMAL,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        let image_view =
            context.create_image_view(&image, SHADOW_MAP_FORMAT, vk::ImageAspectFlags::DEPTH, 1)?;

        let sampler = Self::create_comparison_sampler(&context)?;

        Ok(Self {
            context,
            extent,
            format: SHADOW_MAP_FORMAT,
            image,
            image_memory,
            image_view,
            sampler,
        })
    }

    /// Sampler « de comparaison » : au lieu de renvoyer la valeur stockée, le
    /// hardware compare une référence fournie par le shader (la profondeur du
    /// fragment courant) à la valeur du texel, et renvoie 1.0 (passé) ou 0.0
    /// (échoué). Avec un filtrage LINEAR, il moyenne 4 comparaisons voisines :
    /// c'est du PCF 2×2 gratuit, qui adoucit les bords de l'ombre.
    ///
    /// CLAMP_TO_BORDER + bordure blanche (profondeur 1.0) : tout fragment hors du
    /// frustum de la lumière échantillonne la bordure et ressort « éclairé »,
    /// jamais ombré par erreur.
    fn create_comparison_sampler(context: &RenderingContext) -> Result<vk::Sampler> {
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_BORDER)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_BORDER)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_BORDER)
            .border_color(vk::BorderColor::FLOAT_OPAQUE_WHITE)
            .compare_enable(true)
            .compare_op(vk::CompareOp::LESS_OR_EQUAL)
            .anisotropy_enable(false)
            .unnormalized_coordinates(false)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .min_lod(0.0)
            .max_lod(0.0);

        let sampler = unsafe { context.device.create_sampler(&sampler_info, None) }?;
        Ok(sampler)
    }
}

impl Drop for ShadowMap {
    fn drop(&mut self) {
        unsafe {
            let _ = self.context.device.device_wait_idle();
            self.context.device.destroy_sampler(self.sampler, None);
            self.context
                .device
                .destroy_image_view(self.image_view, None);
            self.context.device.destroy_image(self.image, None);
            self.context.device.free_memory(self.image_memory, None);
        }
    }
}
