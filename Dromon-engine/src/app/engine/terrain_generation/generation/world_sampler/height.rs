//! Altitude d'une colonne, carte macro + détail.
//!
//! `h = sea_level + profil(d) + w(2 − w) · mountain(x) + (1 − w) · fondu(d) · hills(x)`, avec
//! `d` (distance à la côte) et `w` (masque des chaînes) lus sur la carte macro en
//! bicubique (le bilinéaire plisserait le relief tous les `cell_size` voxels). On
//! mélange des **hauteurs**, pas des paramètres (cf. `Architecture/Generation-Monde.md`).

use super::WorldSampler;
use crate::app::engine::terrain_generation::generation::macro_map::coast_profile;
use crate::app::engine::terrain_generation::utils::smoothstep;

impl WorldSampler {
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
}
