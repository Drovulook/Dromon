//! Table des matériaux et **règles de placement** : quel matériau (ou mélange de
//! matériaux) un voxel reçoit selon sa profondeur sous la surface et l'altitude
//! de sa colonne.
//!
//! ## Transition douce entre biomes
//! La couche de surface change de matériau avec l'altitude : sable en bas, herbe
//! au milieu, neige en haut. Un test sur seuil franc (`alt > BORDER`) crée une
//! frontière nette. Ici, autour de chaque bordure on ouvre une **bande de
//! transition** de largeur [`BLEND_WIDTH`] sur laquelle les deux matériaux
//! voisins coexistent en proportions continues, calculées par `smoothstep`.
//!
//! Ces proportions sont stockées dans les canaux `materials`/`weights` du
//! [`Voxel`] (jusqu'à 4 matériaux pondérés). Le rendu les mélange déjà dans
//! `ChunkManager::color_at` ; combiné à l'interpolation du rasterizer entre
//! sommets, on obtient un dégradé doux au lieu d'un trait.

use super::chunk::Voxel;
use super::utils::smoothstep;
use glam::Vec3;

// ─── Table des matériaux ─────────────────────────────────────────────────────
// Matériaux de base (placeholders). À terme, remplacés par une vraie table de
// matériaux (roches, minerais, terre, etc.).
pub const MATERIAL_AIR: u16 = 0; // ID réservé à l'air (absence de matière).
pub const MATERIAL_ROCK: u16 = 1;
pub const MATERIAL_DIRT: u16 = 2;
pub const MATERIAL_GRASS: u16 = 3;
pub const MATERIAL_SNOW: u16 = 4;
pub const MATERIAL_SAND: u16 = 5;

/// Couleur de base (albédo) d'un matériau, par ID.
pub fn material_color(material: u16) -> Vec3 {
    match material {
        MATERIAL_ROCK => Vec3::new(0.40, 0.38, 0.35),
        MATERIAL_DIRT => Vec3::new(0.36, 0.25, 0.16),
        MATERIAL_GRASS => Vec3::new(0.27, 0.42, 0.18),
        MATERIAL_SNOW => Vec3::new(0.95, 0.96, 0.98),
        MATERIAL_SAND => Vec3::new(0.80, 0.73, 0.52),
        _ => Vec3::ZERO, // air / inconnu
    }
}

// ─── Bordures verticales des biomes de surface ───────────────────────────────
/// Altitude (surface monde) de la bordure herbe → neige : au-dessus, neige.
pub const SNOW_BORDER: f64 = 150.0;
/// Altitude de la bordure sable → herbe : en-dessous, sable.
pub const SAND_BORDER: f64 = 10.0;
/// Largeur (en voxels d'altitude) de la bande de transition centrée sur chaque
/// bordure. Sur cette bande, les deux matériaux voisins se mélangent. `0` =
/// frontière nette. Doit rester inférieure à l'écart entre deux bordures pour
/// que trois biomes ne se chevauchent jamais.
pub const BLEND_WIDTH: f64 = 6.0;

/// Profondeur (en voxels sous la surface) de la couche de surface — au-dessous,
/// c'est de la terre puis de la roche.
const SURFACE_DEPTH: f64 = 1.0;
/// Profondeur jusqu'à laquelle on place de la terre ; au-delà, de la roche.
const DIRT_DEPTH: f64 = 4.0;

/// Voxel **plein** à placer à `depth` voxels sous la surface d'une colonne dont
/// l'altitude de surface (déjà perturbée par le bruit de frontière) vaut
/// `mat_alt`. `density` est la valeur signée du champ, déjà calculée par
/// l'appelant. L'air (densité sous l'iso) est géré en amont, pas ici.
///
/// Cascade par profondeur : couche de surface (biome, éventuellement mélangé
/// près d'une bordure) → terre → roche.
pub fn classify_solid(density: f32, depth: f64, mat_alt: f64) -> Voxel {
    if depth < SURFACE_DEPTH {
        let (materials, weights) = surface_blend(mat_alt);
        Voxel {
            density,
            materials,
            weights,
        }
    } else if depth < DIRT_DEPTH {
        Voxel::solid(density, MATERIAL_DIRT)
    } else {
        Voxel::solid(density, MATERIAL_ROCK)
    }
}

/// Composition de la couche de surface à l'altitude `mat_alt` : matériaux + poids
/// quantifiés `[0, 255]`. Loin d'une bordure, un seul matériau à 255 ; dans une
/// bande de transition, les deux biomes voisins en proportions continues.
fn surface_blend(mat_alt: f64) -> ([u16; 4], [u8; 4]) {
    let half = BLEND_WIDTH * 0.5;

    // Fraction de « biome du dessus » de part et d'autre de chaque bordure :
    // 0 sous la bande, 1 au-dessus, courbe en S au milieu.
    let to_grass = smoothstep(SAND_BORDER - half, SAND_BORDER + half, mat_alt);
    let to_snow = smoothstep(SNOW_BORDER - half, SNOW_BORDER + half, mat_alt);

    // Partition de l'unité : poids continu de chaque biome le long de l'altitude.
    // `to_grass` ouvre l'herbe au détriment du sable ; `to_snow` ouvre la neige
    // au détriment de l'herbe. Les deux bordures étant écartées de bien plus que
    // BLEND_WIDTH, au plus deux de ces poids sont non nuls à la fois.
    let layers = [
        (MATERIAL_SAND, 1.0 - to_grass),
        (MATERIAL_GRASS, to_grass * (1.0 - to_snow)),
        (MATERIAL_SNOW, to_snow),
    ];
    pack_weights(&layers)
}

/// Empaquette des couches `(matériau, poids ≥ 0)` dans les 4 canaux d'un voxel,
/// en quantifiant les poids sur `[0, 255]` de somme exacte 255 (proportions
/// relatives). Ignore les couches de poids nul. Suppose au plus 4 couches non
/// nulles (vrai ici : 3 biomes, jamais plus de 2 actifs).
fn pack_weights(layers: &[(u16, f64)]) -> ([u16; 4], [u8; 4]) {
    let mut materials = [MATERIAL_AIR; 4];
    let mut weights = [0u8; 4];

    let total: f64 = layers.iter().map(|&(_, w)| w.max(0.0)).sum();
    if total <= 0.0 {
        return (materials, weights);
    }

    // On remplit les canaux avec les couches non nulles. Le dernier canal rempli
    // reçoit le **reste** (255 − somme des précédents) pour garantir un total de
    // 255 exact malgré les arrondis (sinon la couleur serait légèrement délavée).
    let mut ch = 0;
    let mut assigned = 0u16;
    let mut last = None;
    for &(material, w) in layers {
        if w <= 0.0 || ch >= 4 {
            continue;
        }
        let q = ((w / total) * 255.0).round() as u16;
        materials[ch] = material;
        weights[ch] = q.min(255) as u8;
        assigned += weights[ch] as u16;
        last = Some(ch);
        ch += 1;
    }
    if let Some(i) = last {
        weights[i] = (weights[i] as u16 + 255u16.saturating_sub(assigned)) as u8;
    }

    (materials, weights)
}
