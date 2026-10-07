//! **Carte macro** : grilles 2D précalculées (continents, relief grossier, climat…)
//! que le détail fin lit en interpolant. Permet les calculs non locaux (distance à
//! la côte, ombre pluviométrique, lacs) qu'un bruit point par point ne sait pas
//! faire. Cf. `Architecture/Generation-Monde.md` dans le vault.

mod continents;
mod distance;
mod export;
mod grid;
mod mountains;
mod relief;
mod small_regions;

pub use grid::{GridShape, MacroGrid};

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::app::engine::terrain_generation::CHUNK_SIZE;
use crate::config::WorldGenConfig;
use continents::ContinentField;

/// Toutes les grilles de la carte macro, sur une même [`GridShape`].
pub struct MacroMap {
    shape: GridShape,
    /// Rayon du monde, en voxels.
    radius: f64,
    /// Continentalité brute (sans unité) : seul son signe sert (terre/mer). Les mers
    /// comblées y valent `+ε`, les îlots supprimés `−ε`.
    pub continentality: MacroGrid,
    sea_level: f32,
    max_height: f32,
    /// Distance signée à la côte, en voxels : > 0 terre, < 0 mer.
    pub coast_distance: MacroGrid,
    /// Masque des chaînes `w ∈ [0, 1]` : 1 sur l'axe, 0 en plaine et en mer.
    pub mountains: MacroGrid,
    /// Altitude macro, en voxels : `sea_level + profil(d) + w · amplitude`.
    pub relief: MacroGrid,
    /// Nombre de mers intérieures comblées.
    pub inland_seas_filled: usize,
    /// Nombre de petites îles supprimées.
    pub islands_removed: usize,
}

/// Hash déterministe de `(seed, k)` vers `[0, 1[` (splitmix64).
fn hash01(seed: u32, k: usize) -> f64 {
    let mut z = ((seed as u64) << 32 ^ k as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

impl MacroMap {
    pub fn build(config: &WorldGenConfig) -> MacroMap {
        let m = &config.macro_;
        let radius = (m.world.radius_chunks as usize * CHUNK_SIZE) as f64;
        let shape = GridShape::covering(radius, m.cell_size as f64);

        // ① Continents : forme par le signe du bruit, distance par mesure.
        let field = ContinentField::new(m.seed, m.continents, radius);
        let mut continentality = MacroGrid::from_fn(shape, |x, y| field.value(x, y) as f32);
        let mut land: Vec<bool> = continentality.values().iter().map(|&c| c > 0.0).collect();
        let disc_cells = |r: f64| (std::f64::consts::PI * r * r / shape.cell_size.powi(2)).ceil();
        let c = &m.continents;
        // Mers d'abord : une île dans une mer comblée rejoint la terre qui l'entoure.
        let inland_seas_filled = small_regions::flip_small_regions(
            &mut land,
            shape.n,
            false,
            false,
            disc_cells(c.inland_sea_min_radius) as usize,
            |_| true,
        );
        let islands_removed = small_regions::flip_small_regions(
            &mut land,
            shape.n,
            true,
            true,
            disc_cells(c.island_min_radius) as usize,
            |first| hash01(m.seed, first) >= c.small_island_keep,
        );
        // Régions basculées : `c` = ±ε, pour que son signe suive `land`.
        for (c, &l) in continentality.values_mut().iter_mut().zip(&land) {
            if l != (*c > 0.0) {
                *c = if l { f32::MIN_POSITIVE } else { -f32::MIN_POSITIVE };
            }
        }
        let cell = shape.cell_size as f32;
        let coast_distance = MacroGrid::from_vec(
            shape,
            distance::signed_coast_distance(&land, shape.n)
                .into_iter()
                .map(|d| d * cell)
                .collect(),
        );

        // ② Relief macro : profil côtier + chaînes de montagnes.
        let mountains = mountains::mountain_mask(m.seed, &m.mountains, shape, &coast_distance);
        let relief = MacroGrid::from_vec(
            shape,
            coast_distance
                .values()
                .iter()
                .zip(mountains.values())
                .map(|(&d, &w)| {
                    let h = relief::macro_height(
                        m.world.sea_level,
                        &m.coast,
                        &m.mountains,
                        d as f64,
                        w as f64,
                    );
                    h as f32
                })
                .collect(),
        );

        MacroMap {
            shape,
            radius,
            sea_level: m.world.sea_level as f32,
            max_height: m.world.max_height as f32,
            continentality,
            coast_distance,
            mountains,
            relief,
            inland_seas_filled,
            islands_removed,
        }
    }

    pub fn shape(&self) -> GridShape {
        self.shape
    }

    /// Part de terre dans le disque du monde, dans `[0, 1]`.
    pub fn land_fraction(&self) -> f32 {
        let (mut land, mut total) = (0usize, 0usize);
        for j in 0..self.shape.n {
            for i in 0..self.shape.n {
                let (x, y) = self.shape.cell_center(i, j);
                if x * x + y * y <= self.radius * self.radius {
                    total += 1;
                    land += (self.coast_distance.get(i, j) > 0.0) as usize;
                }
            }
        }
        land as f32 / total.max(1) as f32
    }

    /// Écrit un PNG par champ dans `dir` (créé au besoin). Rend les chemins écrits.
    pub fn export_pngs(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
        let continents = dir.join("continents.png");
        // Isolignes tous les 0,1 de bruit : espacement irrégulier (pente variable).
        export::save_signed(&self.continentality, &continents, 0.6, 0.1)?;
        let coast = dir.join("coast_distance.png");
        // Isolignes tous les 1 000 voxels : espacement régulier (vraie distance).
        export::save_signed(&self.coast_distance, &coast, 6000.0, 1000.0)?;
        let mountains = dir.join("mountains.png");
        export::save_mask(&self.mountains, &self.coast_distance, &mountains)?;
        let relief = dir.join("relief.png");
        export::save_relief(&self.relief, &relief, self.sea_level, self.max_height)?;
        Ok(vec![continents, coast, mountains, relief])
    }
}
