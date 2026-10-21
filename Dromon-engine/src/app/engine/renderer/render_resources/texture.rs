use std::sync::Arc;

use ash::vk;

use crate::app::engine::rendering_context::RenderingContext;

/// Image device-local et sa vue. Le staging qui la remplit n'en fait pas partie : il
/// est rendu à part par `TextureHandler` et libéré après la passe de transfert.
pub struct Texture {
    context: Arc<RenderingContext>,
    pub image_width: u32,
    pub image_height: u32,
    pub mip_levels: u32,
    /// Couches : 1, sauf pour un texture array.
    pub array_layers: u32,
    pub format: vk::Format,
    pub texture_image: vk::Image,
    pub texture_image_memory: vk::DeviceMemory,
    pub texture_image_view: vk::ImageView,
    /// Set propre à la texture : `Some` pour les textures d'objets (un set par
    /// matériau), `None` pour les cartes, regroupées dans un set commun.
    pub descriptor_set: Option<vk::DescriptorSet>,
}

impl Texture {
    pub fn new(
        context: Arc<RenderingContext>,
        image_width: u32,
        image_height: u32,
        mip_levels: u32,
        array_layers: u32,
        format: vk::Format,
        texture_image: vk::Image,
        texture_image_memory: vk::DeviceMemory,
        texture_image_view: vk::ImageView,
        descriptor_set: Option<vk::DescriptorSet>,
    ) -> Texture {
        Texture {
            context,
            image_width,
            image_height,
            mip_levels,
            array_layers,
            format,
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
