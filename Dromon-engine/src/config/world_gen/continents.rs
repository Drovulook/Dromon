//! Section `continents` de `world_macro.ron`.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Forme des continents : `c = base + detail + land_bias − bord`, dont seul le
/// **signe** compte (> 0 terre, < 0 mer). Le bruit de base dessine des masses
/// compactes ; la somme d'octaves découpe les côtes. Le détail déplace la côte
/// d'environ `detail.amplitude / pente de base` : plus il est faible devant la base,
/// plus les continents restent massifs (moins de bras et de presqu'îles).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContinentParams {
    /// Forme des continents : un seul bruit, de longueur d'onde ≈ taille d'un continent.
    pub base: BaseNoise,
    /// Découpe des côtes : somme d'octaves, indépendante de la base.
    pub detail: OctaveNoise,
    /// Ajouté à `base + detail` : > 0 plus de terre, < 0 plus de mer.
    pub land_bias: f64,
    /// Fraction du rayon où l'océan de bord commence à être imposé, dans `[0, 1[`.
    pub border_start: f64,
    /// Retranché au bruit au bord du monde.
    /// `> base.amplitude + detail.amplitude + land_bias` → océan garanti.
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

/// Un bruit simple : valeur dans ~`[-amplitude, amplitude]`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BaseNoise {
    /// Longueur d'onde `1/f`, en voxels.
    pub frequency: f64,
    pub amplitude: f64,
}

impl Default for BaseNoise {
    fn default() -> Self {
        BaseNoise {
            frequency: 0.00004,
            amplitude: 0.6,
        }
    }
}

/// Somme d'octaves (fBm), normalisée par la somme des poids puis multipliée par
/// `amplitude` : valeur dans ~`[-amplitude, amplitude]`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OctaveNoise {
    /// Fréquence de la première octave.
    pub frequency: f64,
    /// `0` = désactivé.
    pub octaves: usize,
    /// Multiplicateur de fréquence entre octaves, non entier (sinon elles s'alignent).
    pub lacunarity: f64,
    /// Multiplicateur de poids entre octaves, dans `]0, 1[` : plus grand = plus de
    /// petits détails.
    pub gain: f64,
    pub amplitude: f64,
}

impl Default for OctaveNoise {
    fn default() -> Self {
        OctaveNoise {
            frequency: 0.0001,
            octaves: 4,
            lacunarity: 2.1,
            gain: 0.6,
            amplitude: 0.3,
        }
    }
}

impl Default for ContinentParams {
    fn default() -> Self {
        ContinentParams {
            base: BaseNoise::default(),
            detail: OctaveNoise::default(),
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
        ensure!(
            self.base.amplitude >= 0.0 && self.detail.amplitude >= 0.0,
            "continents.base.amplitude et detail.amplitude doivent être ≥ 0"
        );
        let d = &self.detail;
        ensure!(
            d.gain > 0.0 && d.gain < 1.0,
            "continents.detail.gain doit être dans ]0, 1["
        );
        ensure!(
            d.octaves <= 1 || d.lacunarity.fract() != 0.0,
            "continents.detail.lacunarity doit être non entière (ici {})",
            d.lacunarity
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
