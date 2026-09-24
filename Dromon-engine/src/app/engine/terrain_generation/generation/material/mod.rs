//! Matériaux du terrain : table des matériaux et **pipeline de placement**.
//!
//! Le matériau d'un point se calcule comme une pile de calques :
//! 1. [`MaterialQuery`] rassemble une fois les entrées (profondeur, altitude
//!    perturbée par le jitter, normale) ;
//! 2. une couche de base pose les strates et la couverture selon l'altitude ;
//! 3. des étapes successives recouvrent une part du mélange ([`MaterialMix::overlay`]) ;
//! 4. le résultat n'est quantifié en [`Voxel`] (4 canaux `u8`) qu'à la fin.
//!
//! Détails : [`rules`] (les étapes), [`mix`] (le mélange de travail).

mod mix;
mod rules;

use super::super::chunk::Voxel;
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
/// Nombre d'IDs de matériau (air compris) : taille de [`mix::MaterialMix`].
pub const MATERIAL_COUNT: usize = 6;

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

// ─── Jitter des frontières d'altitude ────────────────────────────────────────
/// Amplitude du jitter d'altitude pour les sommets d'iso-surface.
pub const SURFACE_JITTER_AMP: f64 = 60.0;
/// Amplitude du jitter d'altitude pour les parois de bordure et le fond.
pub const VOLUME_JITTER_AMP: f64 = 40.0;

/// Rayon (en voxels) du voisinage sur lequel on mesure la pente macro : les reliefs
/// plus étroits que `2 × rayon` n'y comptent pas.
pub const MACRO_SLOPE_RADIUS: f64 = 12.0;

// ─── Pipeline ────────────────────────────────────────────────────────────────
/// Entrées des règles de matériau pour un point. Calculé une fois, lu par toutes
/// les étapes.
pub struct MaterialQuery {
    /// Position monde du point.
    #[allow(dead_code)] // pas encore lue ; servira aux futures règles
    pub pos: Vec3,
    /// Profondeur sous le relief (0 sur l'iso-surface).
    pub depth: f64,
    /// Altitude testée contre les frontières d'altitude, jitter inclus.
    pub mat_alt: f64,
    /// Normale de la surface exposée ; `None` pour les parois et le fond.
    pub normal: Option<Vec3>,
    /// Cosinus de la pente macro (moyenne sur [`MACRO_SLOPE_RADIUS`]) ; `None` hors
    /// surface exposée.
    pub macro_up: Option<f64>,
}

/// Matériau (ou mélange) **plein** d'un point : couche de base, puis
/// modificateurs successifs. L'ordre compte : une étape tardive recouvre les
/// précédentes.
pub fn evaluate(q: &MaterialQuery) -> Voxel {
    let mut mix = rules::base_layer(q);
    rules::slope_rock(q, &mut mix);
    mix.to_voxel()
}
