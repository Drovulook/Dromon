//! Cartes générées → PNG au format des textures du terrain (lues par
//! `terrain_textures.rs`) : couleur sRGB, hauteur 16 bits normalisée, normale NormalGL,
//! AO.

use super::gpu::GeneratedMaps;
use crate::config::{TEXTURE_AO, TEXTURE_COLOR, TEXTURE_HEIGHT, TEXTURE_NORMAL};
use anyhow::{Context, Result};
use image::{DynamicImage, GrayImage, ImageBuffer, Luma, RgbImage};
use rayon::prelude::*;
use std::path::Path;

/// Plage de hauteur de la recette, en unités de tuile, avant normalisation du PNG.
pub(super) struct HeightRange {
    pub min: f32,
    pub max: f32,
}

/// Écrit les 4 PNG dans `dir` (créé au besoin).
pub(super) fn export(maps: &GeneratedMaps, dir: &Path) -> Result<HeightRange> {
    std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
    let size = maps.size;
    let (min, max) = maps
        .height
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &h| {
            (lo.min(h), hi.max(h))
        });
    let span = max - min;

    // Hauteur étalée sur toute la plage 16 bits : le mélange par hauteur compare des
    // hauteurs relatives. Tuile plate → gris moyen.
    let height: Vec<u16> = maps
        .height
        .iter()
        .map(|&h| unorm16(if span > 0.0 { (h - min) / span } else { 0.5 }))
        .collect();
    let color: Vec<u8> = maps
        .albedo
        .iter()
        .flat_map(|c| [0, 1, 2].map(|i| unorm8(linear_to_srgb(c[i]))))
        .collect();
    // [-1, 1] → [0, 1].
    let normal: Vec<u8> = maps
        .normal_ao
        .iter()
        .flat_map(|n| [0, 1, 2].map(|i| unorm8(n[i] * 0.5 + 0.5)))
        .collect();
    let ao: Vec<u8> = maps.normal_ao.iter().map(|n| unorm8(n[3])).collect();

    let images: Vec<(&str, DynamicImage)> = vec![
        (
            TEXTURE_HEIGHT,
            ImageBuffer::<Luma<u16>, _>::from_raw(size, size, height)
                .context("hauteur : taille incohérente")?
                .into(),
        ),
        (
            TEXTURE_COLOR,
            RgbImage::from_raw(size, size, color)
                .context("couleur : taille incohérente")?
                .into(),
        ),
        (
            TEXTURE_NORMAL,
            RgbImage::from_raw(size, size, normal)
                .context("normale : taille incohérente")?
                .into(),
        ),
        (
            TEXTURE_AO,
            GrayImage::from_raw(size, size, ao)
                .context("AO : taille incohérente")?
                .into(),
        ),
    ];
    // L'encodage PNG est le poste lent : un fichier par cœur.
    images.into_par_iter().try_for_each(|(file, image)| {
        let path = dir.join(file);
        image
            .save(&path)
            .with_context(|| format!("écriture de {}", path.display()))
    })?;
    Ok(HeightRange { min, max })
}

/// Encodage sRGB d'une composante linéaire dans [0, 1].
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn unorm8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn unorm16(x: f32) -> u16 {
    (x.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16
}
