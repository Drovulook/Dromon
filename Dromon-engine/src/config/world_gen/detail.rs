//! `world_detail.ron` : le détail calculé à la demande.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::config::ConfigFile;

/// `world_detail.ron` : paramètres du détail calculé à la demande. Hauteur d'une
/// colonne : `sea_level + profil(d) + w(2 − w) · mountain + (1 − w) · fondu(d) · hills`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldDetailConfig {
    /// Relief des chaînes, pondéré par le masque macro `w`.
    pub mountain: HeightParams,
    /// Collines des plaines, pondérées par `1 − w`. Doivent rester ≥ 0 (cf.
    /// `hills_coast`) : `base_height ≥ amplitude`.
    pub hills: HeightParams,
    /// Raccord des collines à la côte : pente douce ou falaise.
    pub hills_coast: HillsCoastParams,
}

/// Raccord des collines à la côte. La côte est soit douce (`fade`), soit en
/// falaise (`cliff_*`) là où un bruit basse fréquence dépasse `cliff_threshold`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HillsCoastParams {
    /// Les collines s'effacent à la côte et atteignent leur plein à cette distance
    /// (voxels) : sinon leurs creux passeraient sous la mer près des côtes basses.
    /// Pente max ≈ 1,5 · hauteur / `fade`.
    pub fade: f64,
    /// Largeur (voxels) d'une falaise à pleine hauteur : basse → à pic. En bord de
    /// zone, la falaise garde sa raideur et perd de la hauteur.
    pub cliff_fade: f64,
    /// Fréquence du bruit qui place les zones de falaises (longueur d'onde `1/f`,
    /// ≫ `fade` pour que la transition reste douce le long de la côte).
    pub cliff_frequency: f64,
    /// Seuil du bruit (∈ ~`[-1, 1]`) au-dessus duquel la côte devient falaise. Plus
    /// haut = plus rare ; ≥ 1.0 = aucune falaise.
    pub cliff_threshold: f64,
}

impl Default for HillsCoastParams {
    fn default() -> Self {
        HillsCoastParams {
            fade: 500.0,
            cliff_fade: 100.0,
            cliff_frequency: 0.0001,
            cliff_threshold: 1.0,
        }
    }
}

impl HillsCoastParams {
    fn validate(&self) -> Result<()> {
        ensure!(self.fade > 0.0, "hills_coast.fade doit être > 0");
        ensure!(self.cliff_fade > 0.0, "hills_coast.cliff_fade doit être > 0");
        ensure!(self.cliff_frequency > 0.0, "hills_coast.cliff_frequency doit être > 0");
        Ok(())
    }
}

impl Default for WorldDetailConfig {
    fn default() -> Self {
        WorldDetailConfig {
            mountain: HeightParams::default(),
            hills: HeightParams::default(),
            hills_coast: HillsCoastParams::default(),
        }
    }
}

impl ConfigFile for WorldDetailConfig {
    const FILE_NAME: &'static str = "world_detail.ron";

    // HACK: il faudrait une macro qui envoie un message au CLI avant de planter
    fn validate(&self) -> Result<()> {
        self.mountain.validate("mountain")?;
        self.hills.validate("hills")?;
        self.hills_coast.validate()
    }
}

/// Paramètres d'un champ d'altitude (`HeightField`) : contrôle du fBm et de l'érosion.
/// Hauteur **relative** au relief macro : `base_height + amplitude · fBm`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeightParams {
    /// Décalage du relief, en voxels (le bruit oscille autour).
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

impl HeightParams {
    fn validate(&self, name: &str) -> Result<()> {
        ensure!(self.octaves >= 1, "{name}.octaves doit être ≥ 1");
        ensure!(
            self.gain > 0.0 && self.gain < 1.0,
            "{name}.gain doit être dans ]0, 1["
        );
        // Lacunarité entière : les réseaux des octaves s'alignent (motifs répétés).
        ensure!(
            self.lacunarity.fract() != 0.0,
            "{name}.lacunarity doit être non entière (ici {})",
            self.lacunarity
        );
        Ok(())
    }
}
