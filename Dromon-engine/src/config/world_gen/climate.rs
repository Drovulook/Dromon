//! Section `climate` de `world_macro.ron` : température, vent, courants.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Climat, calculé sur une grille plus grossière que la carte macro (il varie à
/// l'échelle du km, et l'humidité demande des centaines d'itérations).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClimateParams {
    /// Côté d'une cellule climatique (voxels), multiple de `cell_size`.
    pub cell_size: u32,

    // ─── Température ───
    /// Température au niveau de la mer au bord sud / nord du monde (°C).
    pub south_temperature: f64,
    pub north_temperature: f64,
    /// Refroidissement avec l'altitude au-dessus de la mer, en °C par voxel. Réglé
    /// pour le jeu (le réel, ~0,0065 °C/m, ne ferait jamais neiger 700 voxels).
    pub lapse_rate: f64,
    /// Variations locales (°C) et leur fréquence.
    pub noise_amplitude: f64,
    pub noise_frequency: f64,

    // ─── Vent ───
    /// Vent dominant selon la latitude : points `(latitude, d'où il vient, vitesse)`,
    /// latitude de 0 (bord sud) à 1 (bord nord), strictement croissante ; direction en
    /// cap boussole (0 nord, 90 est, 180 sud, 270 ouest = « vent d'ouest »). Les
    /// **vecteurs** sont interpolés, pas les angles : deux régimes opposés se
    /// compensent en une bande calme (latitudes subtropicales).
    pub wind_bands: Vec<(f64, f64, f64)>,
    /// Rotation maximale de la direction par le bruit (degrés).
    pub wind_deviation_deg: f64,
    /// Variation relative de la vitesse, dans `[0, 1[` (0.3 → ±30 %).
    pub wind_speed_variation: f64,
    /// Fréquence du bruit du vent. Très basse : un vent qui tourne vite strie
    /// l'humidité.
    pub wind_noise_frequency: f64,

    // ─── Courants marins ───
    /// Distance (voxels) sur laquelle l'eau garde la température de son point de
    /// départ : portée d'un courant chaud ou froid. `0.0` = pas de courants (SST à
    /// l'équilibre de sa latitude).
    pub current_reach: f64,
    /// Distance à la côte (voxels) sous laquelle les courants sont déviés pour la
    /// longer au lieu d'y entrer, et où joue la remontée d'eau froide.
    pub current_coast_width: f64,
    /// Remontée d'eau froide (°C) là où le vent pousse l'eau vers le large, à vent
    /// nominal et contre la côte. `0.0` = désactivée.
    pub upwelling_strength: f64,

    // ─── Humidité ───
    /// Recharge en vapeur au-dessus de la mer, par itération : fraction de l'écart à
    /// la saturation à la SST, dans `]0, 1]`.
    pub evaporation: f64,
    /// Pluie de base sur terre, fraction de la vapeur par itération (≈ une cellule
    /// climatique parcourue à vent nominal), dans `]0, 1[`. Assèche l'intérieur des
    /// continents : la vapeur est divisée par e tous les ~`cell_size / rain_base`
    /// voxels.
    pub rain_base: f64,
    /// Pluie orographique : fraction de la vapeur tombée par 100 voxels de montée de
    /// l'air (versant au vent). Crée l'ombre pluviométrique derrière les chaînes.
    pub rain_orographic: f64,
    /// Diffusion de la vapeur par itération, dans `[0, 1]` : apporte un peu
    /// d'humidité aux côtes même sans vent (bande calme).
    pub moisture_diffusion: f64,
}

impl Default for ClimateParams {
    fn default() -> Self {
        ClimateParams {
            cell_size: 256,
            south_temperature: 28.0,
            north_temperature: 2.0,
            lapse_rate: 0.03,
            noise_amplitude: 2.0,
            noise_frequency: 0.00005,
            wind_bands: vec![
                (0.0, 60.0, 1.0),
                (0.35, 60.0, 0.9),
                (0.55, 240.0, 0.9),
                (1.0, 240.0, 1.0),
            ],
            wind_deviation_deg: 30.0,
            wind_speed_variation: 0.3,
            wind_noise_frequency: 0.00002,
            current_reach: 12000.0,
            current_coast_width: 3000.0,
            upwelling_strength: 5.0,
            evaporation: 0.2,
            rain_base: 0.02,
            rain_orographic: 1.0,
            moisture_diffusion: 0.1,
        }
    }
}

impl ClimateParams {
    /// `macro_cell_size` : `cell_size` de la carte macro, dont la grille climatique
    /// doit être un multiple.
    pub(super) fn validate(&self, macro_cell_size: u32) -> Result<()> {
        let cc = self.cell_size;
        ensure!(
            cc >= macro_cell_size && cc % macro_cell_size == 0,
            "climate.cell_size ({cc}) doit être un multiple de cell_size ({macro_cell_size})"
        );
        let bands = &self.wind_bands;
        ensure!(!bands.is_empty(), "climate.wind_bands ne doit pas être vide");
        ensure!(
            bands.windows(2).all(|w| w[0].0 < w[1].0),
            "climate.wind_bands : les latitudes doivent être strictement croissantes"
        );
        ensure!(
            (0.0..1.0).contains(&self.wind_speed_variation),
            "climate.wind_speed_variation doit être dans [0, 1["
        );
        ensure!(
            self.current_reach >= 0.0 && self.current_coast_width > 0.0,
            "climate.current_reach doit être ≥ 0 et current_coast_width > 0"
        );
        ensure!(
            self.upwelling_strength >= 0.0,
            "climate.upwelling_strength doit être ≥ 0"
        );
        ensure!(
            self.evaporation > 0.0 && self.evaporation <= 1.0,
            "climate.evaporation doit être dans ]0, 1]"
        );
        ensure!(
            self.rain_base > 0.0 && self.rain_base < 1.0,
            "climate.rain_base doit être dans ]0, 1["
        );
        ensure!(
            self.rain_orographic >= 0.0,
            "climate.rain_orographic doit être ≥ 0"
        );
        ensure!(
            (0.0..=1.0).contains(&self.moisture_diffusion),
            "climate.moisture_diffusion doit être dans [0, 1]"
        );
        Ok(())
    }
}
