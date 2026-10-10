//! **Relief du terrain** : altitude d'une colonne, carte macro + détail.
//!
//! `h = sea_level + profil(d) + w(2 − w) · mountain(x) + (1 − w) · fondu(d) · hills(x)`, avec
//! `d` (distance à la côte) et `w` (masque des chaînes) lus sur la carte macro en
//! bicubique (le bilinéaire plisserait le relief tous les `cell_size` voxels). On
//! mélange des **hauteurs**, pas des paramètres (cf. `Architecture/Generation-Monde.md`).

use super::height_field::HeightField;
use super::macro_map::{MacroMap, coast_profile};
use crate::app::engine::terrain_generation::utils::smoothstep;
use crate::config::{CoastProfile, WorldGenConfig};

pub struct Relief {
    macro_map: MacroMap,
    mountain: HeightField,
    hills: HeightField,
    sea_level: f64,
    coast: CoastProfile,
    hills_coast_fade: f64,
}

impl Relief {
    /// `macro_map` doit avoir été construite depuis `config` (cf.
    /// [`MacroMap::load_or_build`]).
    pub fn new(config: &WorldGenConfig, macro_map: MacroMap) -> Relief {
        let seed = config.macro_.seed;
        Relief {
            macro_map,
            mountain: HeightField::new(seed, config.detail.mountain),
            // Graine décalée : collines décorrélées des montagnes.
            hills: HeightField::new(seed.wrapping_add(1), config.detail.hills),
            sea_level: config.macro_.world.sea_level,
            coast: config.macro_.coast.clone(),
            hills_coast_fade: config.detail.hills_coast_fade,
        }
    }

    /// Altitude continue (en voxels) à la colonne monde `(wx, wy)`.
    pub fn height(&self, wx: f64, wy: f64) -> f64 {
        let d = self.coast_distance(wx, wy);
        // Catmull-Rom déborde légèrement de [0, 1] au pied des chaînes.
        let w = (self.macro_map.mountains.sample_bicubic(wx, wy) as f64).clamp(0.0, 1.0);
        let hills = (1.0 - w) * smoothstep(0.0, self.hills_coast_fade, d);

        let mut h = self.sea_level + coast_profile(&self.coast, d);
        // `> 0.0`, pas `> ε` : un seuil créerait une marche de `ε × hauteur`. Saute le
        // fBm (le plus cher) en plaine, en mer, et les collines sur l'axe des chaînes.
        if w > 0.0 {
            // `w·(2 − w)` plutôt que `w` : le relief rugueux démarre plus tôt sur le
            // piémont (0,44 à w = 0,25) au lieu d'y faire une rampe lisse. Pente nulle
            // en w = 0, comme `w` (pas `√w` : pente infinie → pli au bord du piémont).
            h += w * (2.0 - w) * self.mountain.height(wx, wy);
        }
        if hills > 0.0 {
            h += hills * self.hills.height(wx, wy);
        }
        h
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
