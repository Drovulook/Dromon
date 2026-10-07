//! Génération du monde, en deux fichiers :
//! - `world_macro.ron` : la **carte macro** précalculée (graine, étendue, grille) ;
//! - `world_detail.ron` : le **détail fin** calculé à la demande (relief fBm…).
//!
//! Le détail lit la macro, jamais l'inverse : les champs communs (graine, étendue)
//! vivent donc côté macro. Modifier `world_detail.ron` n'invalide pas la carte macro.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use super::ConfigFile;

/// Les deux fichiers réunis : ce que la scène passe au terrain.
#[derive(Clone, Debug, Default)]
pub struct WorldGenConfig {
    // `macro` est un mot-clé Rust, d'où le `_` final.
    pub macro_: WorldMacroConfig,
    pub detail: WorldDetailConfig,
}

impl WorldGenConfig {
    /// Charge et valide les deux fichiers par défaut.
    pub fn load() -> Result<WorldGenConfig> {
        Ok(WorldGenConfig {
            macro_: WorldMacroConfig::load()?,
            detail: WorldDetailConfig::load()?,
        })
    }
}

/// `world_macro.ron` : entrées de la carte macro.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldMacroConfig {
    pub seed: u32,
    pub world: WorldShape,
    /// Côté d'une cellule de la carte macro, en voxels.
    pub cell_size: u32,
    pub continents: ContinentParams,
    pub coast: CoastProfile,
    pub mountains: MountainParams,
}

impl Default for WorldMacroConfig {
    fn default() -> Self {
        WorldMacroConfig {
            seed: 0,
            world: WorldShape::default(),
            cell_size: 64,
            continents: ContinentParams::default(),
            coast: CoastProfile::default(),
            mountains: MountainParams::default(),
        }
    }
}

/// Altitude de base selon la distance à la côte `d` (voxels, > 0 terre).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoastProfile {
    /// Points `(d, altitude relative à sea_level)`, `d` strictement croissant, reliés
    /// par des segments ; constant au-delà des extrémités.
    pub points: Vec<(f64, f64)>,
}

impl Default for CoastProfile {
    fn default() -> Self {
        CoastProfile {
            points: vec![
                (-12000.0, -170.0),
                (-4000.0, -140.0),
                (-1500.0, -30.0),
                (0.0, 0.0),
                (3000.0, 30.0),
            ],
        }
    }
}

/// Chaînes de montagnes : où (axes), sur quelle largeur (piémont) et à quelle hauteur.
/// La **forme** du relief montagneux est côté détail.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MountainParams {
    /// Fréquence du bruit ridged dont les crêtes dessinent les axes des chaînes.
    pub frequency: f64,
    /// Axe de chaîne là où `1 − |bruit| > axis_threshold`, dans `[0, 1[`. Plus haut =
    /// chaînes plus fines et plus rares.
    pub axis_threshold: f64,
    /// Domain warping des axes : chaînes pliées plutôt que régulières.
    pub warp_frequency: f64,
    /// Déplacement maximal en voxels. `0.0` = désactivé.
    pub warp_amplitude: f64,
    /// Pas d'axe à moins de cette distance de la côte (voxels).
    pub min_coast_distance: f64,
    /// Largeur du piémont (voxels) : le masque passe de 1 sur l'axe à 0 à cette
    /// distance. Pente ajoutée ≈ 1,5 · amplitude / largeur.
    pub foothill_width: f64,
    /// Le masque s'annule à la côte et atteint son plein à cette distance (voxels) :
    /// les montagnes ne déplacent pas le trait de côte.
    pub coast_fade: f64,
    /// Hauteur des montagnes au-dessus du profil côtier (voxels).
    pub amplitude: f64,
}

impl Default for MountainParams {
    fn default() -> Self {
        MountainParams {
            frequency: 0.00003,
            axis_threshold: 0.9,
            warp_frequency: 0.00006,
            warp_amplitude: 3000.0,
            min_coast_distance: 1500.0,
            foothill_width: 4000.0,
            coast_fade: 1500.0,
            amplitude: 700.0,
        }
    }
}

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

impl ConfigFile for WorldMacroConfig {
    const FILE_NAME: &'static str = "world_macro.ron";

    fn validate(&self) -> Result<()> {
        ensure!(
            self.world.radius_chunks > 0,
            "world.radius_chunks doit être > 0"
        );
        ensure!(self.world.max_height >= 2, "world.max_height doit être ≥ 2");
        ensure!(self.cell_size >= 1, "cell_size doit être ≥ 1");
        let c = &self.continents;
        ensure!(c.octaves >= 1, "continents.octaves doit être ≥ 1");
        ensure!(
            c.gain > 0.0 && c.gain < 1.0,
            "continents.gain doit être dans ]0, 1["
        );
        ensure!(
            c.lacunarity.fract() != 0.0,
            "continents.lacunarity doit être non entière (ici {})",
            c.lacunarity
        );
        ensure!(
            (0.0..1.0).contains(&c.border_start),
            "continents.border_start doit être dans [0, 1["
        );
        ensure!(
            c.border_noise >= 0.0,
            "continents.border_noise doit être ≥ 0"
        );
        ensure!(
            c.inland_sea_min_radius >= 0.0,
            "continents.inland_sea_min_radius doit être ≥ 0"
        );
        ensure!(
            c.island_min_radius >= 0.0,
            "continents.island_min_radius doit être ≥ 0"
        );
        ensure!(
            (0.0..=1.0).contains(&c.small_island_keep),
            "continents.small_island_keep doit être dans [0, 1]"
        );
        let sea = self.world.sea_level;
        ensure!(
            sea >= 0.0 && sea < self.world.max_height as f64,
            "world.sea_level doit être dans [0, max_height["
        );
        let p = &self.coast.points;
        ensure!(!p.is_empty(), "coast.points ne doit pas être vide");
        ensure!(
            p.windows(2).all(|w| w[0].0 < w[1].0),
            "coast.points : les distances doivent être strictement croissantes"
        );
        let m = &self.mountains;
        ensure!(
            (0.0..1.0).contains(&m.axis_threshold),
            "mountains.axis_threshold doit être dans [0, 1["
        );
        ensure!(
            m.foothill_width > 0.0 && m.coast_fade > 0.0,
            "mountains.foothill_width et coast_fade doivent être > 0"
        );
        Ok(())
    }
}

/// `world_detail.ron` : paramètres du détail calculé à la demande.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldDetailConfig {
    /// Réglages du fBm érodé (cf. [`HeightParams`]).
    pub relief: HeightParams,
}

impl ConfigFile for WorldDetailConfig {
    const FILE_NAME: &'static str = "world_detail.ron";

    // HACK: il faudrait une macro qui envoie un message au CLI avant de planter
    fn validate(&self) -> Result<()> {
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
    /// Niveau de la mer, en voxels.
    pub sea_level: f64,
}

impl Default for WorldShape {
    fn default() -> Self {
        WorldShape {
            radius_chunks: 4,
            max_height: 1024,
            sea_level: 200.0,
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
