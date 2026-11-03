use anyhow::{Context, Result, ensure};
use ash::vk;
use glam::Vec3;
use image::{DynamicImage, ImageReader};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::renderer::render_resources::texture::Texture;
use crate::app::engine::renderer::render_resources::texture_handler::TextureHandler;
use crate::app::logger::Logger;
use crate::config::{
    MaterialsConfig, TEXTURE_AO, TEXTURE_COLOR, TEXTURE_HEIGHT, TEXTURE_NORMAL, TextureSource,
    has_all_maps,
};
use crate::profile;

/// Une couche des deux texture arrays, texels RGBA8 rangés ligne par ligne.
struct Layer {
    width: u32,
    height: u32,
    /// rgb = albédo (sRGB), a = hauteur.
    albedo_height: Vec<u8>,
    /// rg = normale x, y (espace tangent, NormalGL), b = AO, a inutilisé (255).
    normal_ao: Vec<u8>,
    /// Albédo moyen, **linéaire** : ce vers quoi tendent les mipmaps (le blit filtre
    /// les formats sRGB en linéaire).
    mean: Vec3,
}

impl Layer {
    /// Couche neutre (aucune texture) : le descriptor doit pointer sur une image valide.
    fn neutral() -> Layer {
        Layer {
            width: 1,
            height: 1,
            albedo_height: vec![255; 4],
            normal_ao: vec![128, 128, 255, 255],
            mean: Vec3::ONE,
        }
    }

    /// Albédo (`color.png`) + hauteur (`height.png`, 16 bits ramenés à 8) en alpha ;
    /// normale (`normal.png`, z abandonné : reconstruit dans le shader) + AO (`ao.png`).
    fn load(dir: &Path) -> Result<Layer> {
        let mut rgba = open(dir, TEXTURE_COLOR)?.to_rgba8();
        let relief = open(dir, TEXTURE_HEIGHT)?.to_luma16();
        let normal = open(dir, TEXTURE_NORMAL)?.to_rgb8();
        let ao = open(dir, TEXTURE_AO)?.to_luma8();
        let dimensions = rgba.dimensions();
        for (file, d) in [
            (TEXTURE_HEIGHT, relief.dimensions()),
            (TEXTURE_NORMAL, normal.dimensions()),
            (TEXTURE_AO, ao.dimensions()),
        ] {
            ensure!(
                d == dimensions,
                "{TEXTURE_COLOR} et {file} de tailles différentes"
            );
        }

        let to_linear: [f64; 256] = std::array::from_fn(|c| srgb_to_linear(c as f64 / 255.0));
        let mut sum = [0.0f64; 3];
        for (px, h) in rgba.pixels_mut().zip(relief.pixels()) {
            for (s, &c) in sum.iter_mut().zip(&px.0[..3]) {
                *s += to_linear[c as usize];
            }
            // 0..=65535 → 0..=255, arrondi (65535 / 255 = 257).
            px[3] = ((h[0] as u32 + 128) / 257) as u8;
        }
        let normal_ao = normal
            .pixels()
            .zip(ao.pixels())
            .flat_map(|(n, a)| [n[0], n[1], a[0], 255])
            .collect();

        let (width, height) = dimensions;
        let n = (width as f64) * (height as f64);
        Ok(Layer {
            width,
            height,
            albedo_height: rgba.into_raw(),
            normal_ao,
            mean: Vec3::new(
                (sum[0] / n) as f32,
                (sum[1] / n) as f32,
                (sum[2] / n) as f32,
            ),
        })
    }
}

/// Décodage sRGB → linéaire d'une composante dans [0, 1].
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn open(dir: &Path, file: &str) -> Result<DynamicImage> {
    let path = dir.join(file);
    let decode = || -> Result<DynamicImage> { Ok(ImageReader::open(&path)?.decode()?) };
    decode().with_context(|| format!("lecture de {}", path.display()))
}

/// Premier dossier complet de `source` (cf. `TextureSource::candidate_dirs`). Repli
/// sur un dossier suivant ou absence → avertissement.
fn find_dir(source: &TextureSource, logger: &Logger) -> Option<PathBuf> {
    let candidates = source.candidate_dirs();
    let index = candidates.iter().position(|dir| has_all_maps(dir));
    match index {
        Some(0) => {}
        Some(i) => logger.warn(&format!(
            "texture {source:?} : cartes absentes ou incomplètes dans {} → repli sur {}",
            candidates[0].display(),
            candidates[i].display()
        )),
        None => {
            let dirs: Vec<String> = candidates.iter().map(|d| d.display().to_string()).collect();
            warn_plain(
                logger,
                source,
                &format!("cartes absentes ou incomplètes dans {}", dirs.join(", ")),
            );
        }
    }
    index.map(|i| candidates[i].clone())
}

/// Texture abandonnée : le matériau garde sa couleur unie.
fn warn_plain(logger: &Logger, source: &TextureSource, reason: &str) {
    logger.warn(&format!("texture {source:?} : {reason} → couleur unie"));
}

/// Garde les couches de la taille la plus fréquente (à égalité, la première rencontrée),
/// les autres passent en couleur unie : les couches d'un texture array ont toutes la
/// même taille. Rend les textures gardées et leurs couches, dans l'ordre.
fn same_size(
    loaded: Vec<(&TextureSource, Layer)>,
    logger: &Logger,
) -> (Vec<TextureSource>, Vec<Layer>) {
    let size = |l: &Layer| (l.width, l.height);
    let mut best = None;
    let mut best_count = 0;
    for (_, layer) in &loaded {
        let count = loaded.iter().filter(|(_, l)| size(l) == size(layer)).count();
        if count > best_count {
            (best, best_count) = (Some(size(layer)), count);
        }
    }
    let mut sources = Vec::new();
    let mut layers = Vec::new();
    for (source, layer) in loaded {
        match best {
            Some(best) if size(&layer) != best => warn_plain(
                logger,
                source,
                &format!(
                    "{}×{}, autres textures en {}×{} (couches d'un texture array de même \
                     taille)",
                    layer.width, layer.height, best.0, best.1
                ),
            ),
            _ => {
                sources.push(source.clone());
                layers.push(layer);
            }
        }
    }
    (sources, layers)
}

/// **Textures du terrain** : deux `Texture2DArray` RGBA8 aux mêmes couches, une par
/// texture de `materials.ron` **chargée** (cf. [`Self::layer_of`]).
/// - `albedo` (`_SRGB`) : rgb = albédo, a = hauteur du relief (mélange par hauteur).
///   En `_SRGB`, l'alpha n'est **pas** décodé : la hauteur reste linéaire.
/// - `normal` (`_UNORM` : des vecteurs, pas des couleurs) : rg = normale x, y, b = AO.
///
/// Une texture introuvable ou illisible n'est pas une erreur : le matériau garde sa
/// couleur unie, avec un avertissement. Chargée au démarrage et au hot reload.
pub struct TerrainTextures {
    albedo: Texture,
    normal: Texture,
    /// Texture de chaque couche, dans l'ordre (vide : seule la couche neutre existe).
    sources: Vec<TextureSource>,
    /// Albédo moyen linéaire de chaque couche (cf. `Layer::mean`).
    means: Vec<Vec3>,
    /// Staging de `albedo` puis de `normal`. Vidé par `release_texture_staging`.
    startup_staging: Option<[Buffer; 2]>,
}

impl TerrainTextures {
    pub fn new(texture_handler: &TextureHandler, config: &MaterialsConfig) -> Result<Self> {
        profile!();
        let logger = &texture_handler.logger;
        let found: Vec<(&TextureSource, PathBuf)> = config
            .textures()
            .into_iter()
            .filter_map(|source| Some((source, find_dir(source, logger)?)))
            .collect();
        // Décodage PNG coûteux (4 cartes 2K par couche) : en parallèle.
        let loaded: Vec<(&TextureSource, Layer)> = found
            .par_iter()
            .map(|(source, dir)| (*source, Layer::load(dir)))
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|(source, layer)| match layer {
                Ok(layer) => Some((source, layer)),
                Err(e) => {
                    warn_plain(logger, source, &format!("{e:#}"));
                    None
                }
            })
            .collect();
        let (sources, mut layers) = same_size(loaded, logger);
        if layers.is_empty() {
            // Le descriptor doit pointer sur une image valide.
            layers.push(Layer::neutral());
        }

        let (width, height) = (layers[0].width, layers[0].height);
        let count = layers.len() as u32;
        let create = |texels: fn(&Layer) -> &[u8], format| {
            let bytes = layers.iter().map(texels).collect::<Vec<_>>().concat();
            texture_handler.create_texture_array(width, height, count, format, &bytes)
        };
        let (albedo, albedo_staging) =
            create(|l| &l.albedo_height, vk::Format::R8G8B8A8_SRGB)?;
        let (normal, normal_staging) = create(|l| &l.normal_ao, vk::Format::R8G8B8A8_UNORM)?;
        texture_handler.logger.info(&format!(
            "Textures du terrain : {count} couches {width}×{height} (albédo + normales)"
        ));

        Ok(TerrainTextures {
            albedo,
            normal,
            sources,
            means: layers.iter().map(|l| l.mean).collect(),
            startup_staging: Some([albedo_staging, normal_staging]),
        })
    }

    /// Couche de `source`, `None` si elle n'a pas été chargée (couleur unie).
    pub fn layer_of(&self, source: &TextureSource) -> Option<u32> {
        self.sources
            .iter()
            .position(|s| s == source)
            .map(|i| i as u32)
    }

    /// Albédo + hauteur.
    pub fn albedo_view(&self) -> vk::ImageView {
        self.albedo.texture_image_view
    }

    /// Normale + AO.
    pub fn normal_view(&self) -> vk::ImageView {
        self.normal.texture_image_view
    }

    /// Albédo moyen linéaire de la couche `layer`.
    pub fn mean(&self, layer: u32) -> Vec3 {
        self.means[layer as usize]
    }

    /// Copie staging → device + mipmaps. Passe de transfert du démarrage uniquement.
    pub fn record_upload(
        &self,
        texture_handler: &TextureHandler,
        command_buffer: &vk::CommandBuffer,
    ) -> Result<()> {
        let [albedo_staging, normal_staging] = self
            .startup_staging
            .as_ref()
            .context("textures du terrain déjà envoyées : staging libéré")?;
        texture_handler.initialize(&self.albedo, albedo_staging, command_buffer)?;
        texture_handler.initialize(&self.normal, normal_staging, command_buffer)
    }

    /// Libère le staging. Seulement une fois la passe de transfert **exécutée**.
    pub fn release_texture_staging(&mut self) {
        self.startup_staging = None;
    }
}
