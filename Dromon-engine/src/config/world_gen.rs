//! `world_gen.ron` : génération du monde (graine, étendue, relief).

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use super::ConfigFile;

/// Tous les paramètres de génération du monde.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldGenConfig {
    pub seed: u32,
    pub world: WorldShape,
    /// Réglages du fBm érodé (cf. [`HeightParams`]).
    pub relief: HeightParams,
}

impl ConfigFile for WorldGenConfig {
    const FILE_NAME: &'static str = "world_gen.ron";

    // HACK: il faudrait une macro qui envoie un message au CLI avant de planter
    fn validate(&self) -> Result<()> {
        ensure!(
            self.world.radius_chunks > 0,
            "world.radius_chunks doit être > 0"
        );
        ensure!(self.world.max_height >= 2, "world.max_height doit être ≥ 2");
        let r = &self.relief;
        ensure!(r.octaves >= 1, "relief.octaves doit être ≥ 1");
        ensure!(
            r.gain > 0.0 && r.gain < 1.0,
            "relief.gain doit être dans ]0, 1["
        );
        // Lacunarité entière : les réseaux des octaves s'alignent (motifs répétés).
        ensure!(
            r.lacunarity.fract() != 0.0,
            "relief.lacunarity doit être non entière (ici {})",
            r.lacunarity
        );
        Ok(())
    }
}

/// Étendue du monde.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldShape {
    /// Rayon du monde, en chunks (cf. `WorldDisc`).
    pub radius_chunks: u32,
    /// Plafond du monde, en voxels : aucun relief n'est maillé au-dessus.
    pub max_height: u32,
}

impl Default for WorldShape {
    fn default() -> Self {
        WorldShape {
            radius_chunks: 4,
            max_height: 1024,
        }
    }
}

/// Paramètres du champ d'altitude (`HeightField`) : contrôle du fBm et de l'érosion.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeightParams {
    /// Altitude moyenne du terrain, en voxels (le bruit oscille autour).
    pub base_height: f64,
    /// Amplitude verticale totale du relief, en voxels.
    pub amplitude: f64,
    /// Fréquence de la **première** octave : plus petit = collines plus larges.
    pub frequency: f64,
    /// Nombre d'octaves (couches de détail superposées).
    pub octaves: usize,
    /// Multiplicateur de fréquence entre deux octaves (typiquement ~2.0).
    /// !!! ATTENTION : `lacunarity` doit être non-entière pour éviter un alignement des octaves
    pub lacunarity: f64,
    /// Multiplicateur d'amplitude entre deux octaves, dans `]0, 1[` (la
    /// « persistence » : ~0.5 donne un relief équilibré).
    pub gain: f64,
    /// Force de l'érosion `k`. `0.0` = fBm classique sans érosion ; plus grand =
    /// vallées plus creusées et crêtes plus marquées. Indépendante de `frequency`
    /// et de `amplitude` (gradient pris dans l'espace du bruit).
    pub erosion: f64,
    /// Mélange `[0, 1]` entre bruit doux et bruit « ridged ». `0.0` = collines
    /// arrondies (fBm classique) ; `1.0` = arêtes vives (`1 − |bruit|`), aspect
    /// montagne escarpée. Valeurs intermédiaires = entre les deux.
    pub ridge: f64,
    /// Aplatissement des basses terres `[0, 1]` (effet multifractal). `0.0` =
    /// même rugosité partout ; `1.0` = le détail fin et les arêtes ne sont ajoutés
    /// qu'en altitude, les vallées restent douces et peu pentues (plus réaliste).
    pub lowland_flatness: f64,
    /// Modulation du `ridge` par l'altitude `[0, 1]`. `0.0` = même mélange partout ;
    /// `1.0` = arêtes vives réservées aux hauteurs, vallées entièrement arrondies.
    /// L'amplitude du relief, elle, ne bouge pas (cf. `value_std` dans `height_field.rs`).
    pub ridge_altitude: f64,
    /// Arrondi des arêtes ridged, en unités de bruit (`0.0` = arête vive). Largeur
    /// en voxels = `ridge_smoothness / freq` : large aux grandes octaves (plus
    /// d'arête-lame kilométrique), négligeable aux fines. ~0.05.
    pub ridge_smoothness: f64,

    /// Fréquence de la carte de massifs. Longueur d'onde (`1/f`) de quelques fois
    /// celle de l'octave de base, mais inférieure à la taille du monde.
    pub massif_frequency: f64,
    /// Bande `smoothstep` appliquée au bruit de massifs (∈ ~`[-1, 1]`) : sous
    /// `massif_low` plaine, au-dessus de `massif_high` massif. Étroite = régions
    /// tranchées ; large = gradient mou.
    pub massif_low: f64,
    pub massif_high: f64,
    /// Facteur d'amplitude en plaine `[0, 1]`. `1.0` = carte de massifs désactivée.
    pub massif_min: f64,
    /// Fréquence du champ de déplacement du domain warping (~`frequency`).
    pub warp_frequency: f64,
    /// Déplacement maximal en voxels. `0.0` = warping désactivé. Trop fort (≫ la
    /// longueur d'onde de base) → aspect marbre tourbillonnant.
    pub warp_amplitude: f64,
}

impl Default for HeightParams {
    fn default() -> Self {
        HeightParams {
            base_height: 64.0,
            amplitude: 24.0,
            frequency: 0.02,
            octaves: 5,
            lacunarity: 2.5,
            gain: 0.5,
            erosion: 1.0,
            ridge: 0.0,
            lowland_flatness: 0.0,
            ridge_altitude: 0.0,
            ridge_smoothness: 0.0,
            massif_frequency: 0.002,
            massif_low: -0.1,
            massif_high: 0.2,
            massif_min: 1.0,
            warp_frequency: 0.02,
            warp_amplitude: 0.0,
        }
    }
}
