//! Section `continents` de `world_macro.ron`.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Forme des continents : fBm basse fréquence dont seul le **signe** compte
/// (> 0 terre, < 0 mer), moins un terme qui force l'océan au bord du monde.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContinentParams {
    /// Fréquence de la première octave. Longueur d'onde `1/f` ≈ taille d'un continent.
    pub frequency: f64,
    pub octaves: usize,
    /// Non entière, sinon les octaves s'alignent.
    pub lacunarity: f64,
    /// Dans `]0, 1[` : plus grand = côtes plus découpées.
    pub gain: f64,
    /// Ajouté au bruit (~`[-1, 1]`) : > 0 plus de terre, < 0 plus de mer.
    pub land_bias: f64,
    /// Fraction du rayon où l'océan de bord commence à être imposé, dans `[0, 1[`.
    pub border_start: f64,
    /// Retranché au bruit au bord du monde. > 1 + `land_bias` → océan garanti.
    pub border_strength: f64,
    /// Ondulation du début de l'océan de bord, en fraction du rayon : il commence
    /// entre `border_start − border_noise` et `border_start`. `0.0` = cercle.
    pub border_noise: f64,
    /// Fréquence de cette ondulation. Périmètre `2πR` × f ≈ nombre de caps et de baies.
    pub border_noise_frequency: f64,
    /// Une mer non reliée à l'océan est comblée si sa surface est sous celle d'un
    /// disque de ce rayon (voxels). `0.0` = toutes gardées ; très grand = aucune.
    pub inland_sea_min_radius: f64,
    /// Une île est « petite » si sa surface est sous celle d'un disque de ce rayon
    /// (voxels). `0.0` = aucune.
    pub island_min_radius: f64,
    /// Fraction des petites îles gardées, dans `[0, 1]` (tirage déterministe par la
    /// graine). `1.0` = toutes gardées, `0.0` = toutes supprimées.
    pub small_island_keep: f64,
}

impl Default for ContinentParams {
    fn default() -> Self {
        ContinentParams {
            frequency: 0.00006,
            octaves: 4,
            lacunarity: 2.1,
            gain: 0.5,
            land_bias: 0.0,
            border_start: 0.7,
            border_strength: 1.5,
            border_noise: 0.0,
            border_noise_frequency: 0.0001,
            inland_sea_min_radius: 0.0,
            island_min_radius: 0.0,
            small_island_keep: 1.0,
        }
    }
}

impl ContinentParams {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(self.octaves >= 1, "continents.octaves doit être ≥ 1");
        ensure!(
            self.gain > 0.0 && self.gain < 1.0,
            "continents.gain doit être dans ]0, 1["
        );
        ensure!(
            self.lacunarity.fract() != 0.0,
            "continents.lacunarity doit être non entière (ici {})",
            self.lacunarity
        );
        ensure!(
            (0.0..1.0).contains(&self.border_start),
            "continents.border_start doit être dans [0, 1["
        );
        ensure!(
            self.border_noise >= 0.0,
            "continents.border_noise doit être ≥ 0"
        );
        ensure!(
            self.inland_sea_min_radius >= 0.0,
            "continents.inland_sea_min_radius doit être ≥ 0"
        );
        ensure!(
            self.island_min_radius >= 0.0,
            "continents.island_min_radius doit être ≥ 0"
        );
        ensure!(
            (0.0..=1.0).contains(&self.small_island_keep),
            "continents.small_island_keep doit être dans [0, 1]"
        );
        Ok(())
    }
}
