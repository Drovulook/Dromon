//! **Carte macro** : grilles 2D précalculées (continents, relief grossier, climat…)
//! que le détail fin lit en interpolant. Permet les calculs non locaux (distance à
//! la côte, ombre pluviométrique, lacs) qu'un bruit point par point ne sait pas
//! faire. Cf. `Architecture/Generation-Monde.md` dans le vault.

mod cache;
mod climate;
mod continents;
mod distance;
mod export;
mod grid;
mod moisture;
mod mountains;
mod ocean;
mod relief;
mod small_regions;

pub use cache::MACRO_MAP_CACHE;
pub use grid::{GridShape, MacroGrid};
pub(crate) use relief::coast_profile;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use image::Rgb;

use crate::app::engine::terrain_generation::CHUNK_SIZE;
use crate::config::WorldGenConfig;
use continents::ContinentField;

/// Toutes les grilles de la carte macro, sur une même [`GridShape`].
pub struct MacroMap {
    /// Config qui a produit la carte (cf. `cache::cache_key`).
    cache_key: String,
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
    // ─── Grille climatique (`climate.cell_size`) ───
    /// `relief` et `coast_distance` moyennés par blocs.
    pub climate_relief: MacroGrid,
    pub climate_coast: MacroGrid,
    /// Température imposée par la seule latitude (°C) : équilibre de la SST.
    pub latitude_temperature: MacroGrid,
    /// Température de l'air (°C).
    pub temperature: MacroGrid,
    /// Vent (composantes est et nord), en multiples de la vitesse nominale.
    pub wind_east: MacroGrid,
    pub wind_north: MacroGrid,
    /// Courants marins (est, nord), même unité ; nuls sur terre.
    pub current_east: MacroGrid,
    pub current_north: MacroGrid,
    /// Remontée d'eau froide (≥ 0) : là où le vent pousse l'eau vers le large.
    pub upwelling: MacroGrid,
    /// Température de surface de la mer (°C).
    pub sst: MacroGrid,
    /// Précipitations : 0 = désert, 1 = côte humide de référence, > 1 = versant
    /// exposé (cf. `moisture::precipitation`).
    pub precipitation: MacroGrid,
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
                *c = if l {
                    f32::MIN_POSITIVE
                } else {
                    -f32::MIN_POSITIVE
                };
            }
        }
        let cell = shape.cell_size as f32;
        let coast_distance = MacroGrid::from_vec(shape, distance::signed_coast_distance(&land, shape.n))
            .map(|d| d * cell);

        // ② Relief macro : profil côtier + chaînes de montagnes.
        let mountains = mountains::mountain_mask(m.seed, &m.mountains, shape, &coast_distance);
        let relief = coast_distance.zip_map(&mountains, |d, w| {
            relief::macro_height(m.world.sea_level, &m.coast, &m.mountains, d as f64, w as f64)
                as f32
        });

        // ④ Climat, sur une grille plus grossière.
        let factor = (m.climate.cell_size / m.cell_size) as usize;
        let climate_relief = relief.downsample(factor);
        let climate_coast = coast_distance.downsample(factor);
        let cshape = climate_relief.shape();
        let latitude_temperature = climate::latitude_temperature(&m.climate, radius, cshape);
        let sea_level_temperature =
            climate::sea_level_temperature(m.seed, &m.climate, &latitude_temperature);
        let temperature = climate::temperature(
            &m.climate,
            &sea_level_temperature,
            m.world.sea_level,
            &climate_relief,
        );
        let (wind_east, wind_north) = climate::wind(m.seed, &m.climate, radius, cshape);
        let currents = ocean::currents(&m.climate, &wind_east, &wind_north, &climate_coast);
        let sst = ocean::sea_surface_temperature(&m.climate, &latitude_temperature, &currents);
        let ocean::Currents {
            east: current_east,
            north: current_north,
            upwelling,
        } = currents;
        let precipitation = moisture::precipitation(
            &m.climate,
            &moisture::MoistureInputs {
                wind_east: &wind_east,
                wind_north: &wind_north,
                temperature: &temperature,
                sst: &sst,
                relief: &climate_relief,
                coast: &climate_coast,
            },
        );

        MacroMap {
            cache_key: cache::cache_key(m),
            shape,
            radius,
            sea_level: m.world.sea_level as f32,
            max_height: m.world.max_height as f32,
            continentality,
            coast_distance,
            mountains,
            relief,
            climate_relief,
            climate_coast,
            latitude_temperature,
            temperature,
            wind_east,
            wind_north,
            current_east,
            current_north,
            upwelling,
            sst,
            precipitation,
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
        let temperature = dir.join("temperature.png");
        export::save_temperature(&self.temperature, &self.coast_distance, false, &temperature)?;
        let relief_img = export::relief_image(&self.relief, self.sea_level, self.max_height);
        let wind = dir.join("wind.png");
        export::save_arrows(
            relief_img.clone(),
            &self.wind_east,
            &self.wind_north,
            self.shape,
            None,
            Rgb([20, 20, 20]),
            &wind,
        )?;
        let currents = dir.join("currents.png");
        export::save_arrows(
            relief_img,
            &self.current_east,
            &self.current_north,
            self.shape,
            Some(&self.coast_distance),
            Rgb([255, 255, 255]),
            &currents,
        )?;
        let sst = dir.join("sst.png");
        export::save_temperature(&self.sst, &self.coast_distance, true, &sst)?;
        let anomaly = dir.join("sst_anomaly.png");
        export::save_sst_anomaly(
            &self.sst,
            &self.latitude_temperature,
            &self.coast_distance,
            4.0,
            &anomaly,
        )?;
        let precipitation = dir.join("precipitation.png");
        export::save_precipitation(
            &self.precipitation,
            &self.relief,
            &self.coast_distance,
            &precipitation,
        )?;
        Ok(vec![
            continents,
            coast,
            mountains,
            relief,
            temperature,
            wind,
            currents,
            sst,
            anomaly,
            precipitation,
        ])
    }
}
