//! **Échantillonneur du monde** : tout ce que le détail (⑤) lit en un point, carte
//! macro interpolée + bruits fins. Immuable, partagé entre les threads de maillage.
//!
//! - [`height`] : altitude d'une colonne ;
//! - [`climate`] : température (corrigée par l'altitude) et précipitations.

mod climate;
mod height;

pub use climate::LocalClimate;

use super::height_field::HeightField;
use super::macro_map::MacroMap;
use crate::config::{CoastProfile, WorldGenConfig};

pub struct WorldSampler {
    macro_map: MacroMap,
    mountain: HeightField,
    hills: HeightField,
    sea_level: f64,
    coast: CoastProfile,
    hills_coast_fade: f64,
    /// °C perdus par voxel au-dessus de la mer (cf. `ClimateParams::lapse_rate`).
    lapse_rate: f64,
}

impl WorldSampler {
    /// `macro_map` doit avoir été construite depuis `config` (cf.
    /// [`MacroMap::load_or_build`]).
    pub fn new(config: &WorldGenConfig, macro_map: MacroMap) -> WorldSampler {
        let seed = config.macro_.seed;
        WorldSampler {
            macro_map,
            mountain: HeightField::new(seed, config.detail.mountain),
            // Graine décalée : collines décorrélées des montagnes.
            hills: HeightField::new(seed.wrapping_add(1), config.detail.hills),
            sea_level: config.macro_.world.sea_level,
            coast: config.macro_.coast.clone(),
            hills_coast_fade: config.detail.hills_coast_fade,
            lapse_rate: config.macro_.climate.lapse_rate,
        }
    }

    /// La carte macro sous-jacente.
    pub fn macro_map(&self) -> &MacroMap {
        &self.macro_map
    }

    /// Distance signée à la côte (voxels, > 0 terre) à la colonne `(wx, wy)`.
    pub fn coast_distance(&self, wx: f64, wy: f64) -> f64 {
        self.macro_map.coast_distance.sample_bicubic(wx, wy) as f64
    }

    /// Niveau de la mer, en voxels.
    pub fn sea_level(&self) -> f64 {
        self.sea_level
    }

    /// Cf. [`HeightField::material_jitter`].
    pub fn material_jitter(&self, wx: f64, wy: f64, amp: f64) -> f64 {
        self.mountain.material_jitter(wx, wy, amp)
    }

    /// Cf. [`HeightField::dirt_patch_noise`].
    pub fn dirt_patch_noise(&self, wx: f64, wy: f64) -> f64 {
        self.mountain.dirt_patch_noise(wx, wy)
    }
}
