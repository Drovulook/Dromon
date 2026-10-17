//! Climat local : grilles climatiques de la carte macro, ramenées à l'altitude du point.

use glam::DVec2;

use super::WorldSampler;

/// Climat local d'un point (cf. [`WorldSampler::climate`]).
#[derive(Clone, Copy, Debug)]
pub struct LocalClimate {
    /// Température de l'air (°C), à l'altitude du point.
    pub temperature: f64,
    /// `|∇T|` le long du sol (°C par voxel horizontal) : convertit un écart de
    /// température en distance au sol. Grand en montagne (altitude), minuscule en
    /// plaine (latitude seule).
    pub temperature_gradient: f64,
    /// Précipitations normalisées (1 = côte humide de référence).
    pub precipitation: f64,
    /// `|∇pluie|` (par voxel horizontal) : même rôle que `temperature_gradient`.
    pub precipitation_gradient: f64,
}

impl WorldSampler {
    /// Climat à la colonne `(wx, wy)` et à l'altitude `z` (voxels) ; `height_gradient`
    /// est la pente du sol (`∇h`), pour [`LocalClimate::temperature_gradient`]. Grilles
    /// lues en bilinéaire (le climat varie au km, pas de pli visible).
    ///
    /// La grille donne T à l'altitude **moyenne** de sa cellule. On la ramène au niveau
    /// de la mer (`+ lapse · h_moyenne`, champ lisse), puis à `z` (`− lapse · z`), au-dessus
    /// de la mer seulement, comme la grille. Approximation : le `max(·, 0)` est appliqué
    /// après interpolation de `h_moyenne`, l'écart n'existe qu'à cheval sur la côte.
    pub fn climate(&self, wx: f64, wy: f64, z: f64, height_gradient: DVec2) -> LocalClimate {
        let m = &self.macro_map;
        let above_sea = |h: f64| (h - self.sea_level).max(0.0);
        let sea_temp = |x: f64, y: f64| {
            m.temperature.sample(x, y) as f64
                + self.lapse_rate * above_sea(m.climate_relief.sample(x, y) as f64)
        };

        let rain = |x: f64, y: f64| m.precipitation.sample(x, y) as f64;

        // Différences centrées sur une cellule climatique.
        let s = m.temperature.shape().cell_size;
        let gradient = |f: &dyn Fn(f64, f64) -> f64| {
            DVec2::new(f(wx + s, wy) - f(wx - s, wy), f(wx, wy + s) - f(wx, wy - s)) / (2.0 * s)
        };
        let climate_gradient = gradient(&sea_temp);
        let altitude_gradient = if z > self.sea_level {
            self.lapse_rate * height_gradient
        } else {
            DVec2::ZERO
        };

        LocalClimate {
            temperature: sea_temp(wx, wy) - self.lapse_rate * above_sea(z),
            temperature_gradient: (climate_gradient - altitude_gradient).length(),
            precipitation: rain(wx, wy),
            precipitation_gradient: gradient(&rain).length(),
        }
    }
}
