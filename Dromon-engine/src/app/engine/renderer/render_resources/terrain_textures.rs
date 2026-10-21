use anyhow::{Context, Result, ensure};
use ash::vk;
use image::{DynamicImage, ImageReader};
use rayon::prelude::*;

use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::renderer::render_resources::texture::Texture;
use crate::app::engine::renderer::render_resources::texture_handler::TextureHandler;
use crate::config::{MaterialsConfig, TEXTURE_COLOR, TEXTURE_HEIGHT, terrain_texture_path};
use crate::profile;

/// Une couche du texture array, texels RGBA8 rangés ligne par ligne.
struct Layer {
    width: u32,
    height: u32,
    texels: Vec<u8>,
}

impl Layer {
    /// Albédo (`color.png`) + hauteur (`height.png`, 16 bits ramenés à 8) en alpha.
    fn load(texture: &str) -> Result<Layer> {
        let mut rgba = open(texture, TEXTURE_COLOR)?.to_rgba8();
        let relief = open(texture, TEXTURE_HEIGHT)?.to_luma16();
        ensure!(
            rgba.dimensions() == relief.dimensions(),
            "texture {texture} : {TEXTURE_COLOR} et {TEXTURE_HEIGHT} de tailles différentes"
        );
        for (px, h) in rgba.pixels_mut().zip(relief.pixels()) {
            // 0..=65535 → 0..=255, arrondi (65535 / 255 = 257).
            px[3] = ((h[0] as u32 + 128) / 257) as u8;
        }
        let (width, height) = rgba.dimensions();
        Ok(Layer {
            width,
            height,
            texels: rgba.into_raw(),
        })
    }
}

fn open(texture: &str, file: &str) -> Result<DynamicImage> {
    let path = terrain_texture_path(texture, file);
    let decode = || -> Result<DynamicImage> { Ok(ImageReader::open(&path)?.decode()?) };
    decode().with_context(|| format!("lecture de {}", path.display()))
}

/// **Textures du terrain** : un `Texture2DArray` RGBA8, une couche par texture
/// distincte de `materials.ron` (couche `i` = `MaterialsConfig::textures()[i]`).
/// rgb = albédo (sRGB), a = hauteur du relief (mélange par hauteur). En `_SRGB`,
/// l'alpha n'est **pas** décodé : la hauteur reste linéaire.
///
/// Statique : chargée au démarrage, envoyée dans la passe de transfert.
pub struct TerrainTextures {
    texture: Texture,
    /// Vidé par `release_texture_staging`.
    startup_staging: Option<Buffer>,
}

impl TerrainTextures {
    pub fn new(texture_handler: &TextureHandler, config: &MaterialsConfig) -> Result<Self> {
        profile!();
        let names = config.textures();
        let layers: Vec<Layer> = if names.is_empty() {
            // Le descriptor doit pointer sur une image valide : une couche 1 × 1 blanche.
            vec![Layer {
                width: 1,
                height: 1,
                texels: vec![255; 4],
            }]
        } else {
            // Décodage PNG coûteux (2 cartes 2K par couche) : en parallèle.
            names
                .par_iter()
                .map(|t| Layer::load(t))
                .collect::<Result<_>>()?
        };

        let (width, height) = (layers[0].width, layers[0].height);
        for (name, layer) in names.iter().zip(&layers) {
            ensure!(
                (layer.width, layer.height) == (width, height),
                "texture {name} : {}×{}, attendu {width}×{height} (couches d'un texture array \
                 de même taille)",
                layer.width,
                layer.height,
            );
        }
        let count = layers.len() as u32;
        let texels = layers
            .iter()
            .map(|l| l.texels.as_slice())
            .collect::<Vec<_>>()
            .concat();
        let (texture, staging) = texture_handler.create_texture_array(
            width,
            height,
            count,
            vk::Format::R8G8B8A8_SRGB,
            &texels,
        )?;
        texture_handler.logger.info(&format!(
            "Textures du terrain : {count} couches {width}×{height}"
        ));

        Ok(TerrainTextures {
            texture,
            startup_staging: Some(staging),
        })
    }

    pub fn image_view(&self) -> vk::ImageView {
        self.texture.texture_image_view
    }

    /// Copie staging → device + mipmaps. Passe de transfert du démarrage uniquement.
    pub fn record_upload(
        &self,
        texture_handler: &TextureHandler,
        command_buffer: &vk::CommandBuffer,
    ) -> Result<()> {
        let staging = self
            .startup_staging
            .as_ref()
            .context("textures du terrain déjà envoyées : staging libéré")?;
        texture_handler.initialize(&self.texture, staging, command_buffer)
    }

    /// Libère le staging. Seulement une fois la passe de transfert **exécutée**.
    pub fn release_texture_staging(&mut self) {
        self.startup_staging = None;
    }
}
