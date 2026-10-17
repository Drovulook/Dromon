//! Matériaux du terrain : table des matériaux et **pipeline de placement**.
//!
//! Le matériau d'un point se calcule comme une pile de calques :
//! 1. [`MaterialQuery`] rassemble une fois les entrées (profondeur, altitude
//!    perturbée par le jitter, normale) ;
//! 2. une couche de base pose les strates et la couverture selon l'altitude ;
//! 3. des étapes successives recouvrent une part du mélange ([`MaterialMix::overlay`]) ;
//! 4. le résultat n'est quantifié en [`Voxel`] (3 canaux `u8`) qu'à la fin.
//!
//! Détails : [`rules`] (les étapes), [`mix`] (le mélange de travail), [`noise`] (les
//! bruits des règles), [`table`] (les
//! matériaux de `materials.ron`, résolus en IDs).

mod mix;
mod noise;
mod rules;
mod table;

pub use table::{MaterialIds, MaterialTable};

use super::super::chunk::Voxel;
use super::world_sampler::LocalClimate;
use crate::config::MaterialRulesConfig;
use glam::Vec3;

/// ID réservé à l'air (absence de matière). Les autres viennent de `materials.ron`
/// (cf. [`MaterialTable`]).
pub const MATERIAL_AIR: u16 = 0;

// ─── Pipeline ────────────────────────────────────────────────────────────────
/// Entrées des règles de matériau pour un point. Calculé une fois, lu par toutes
/// les étapes.
pub struct MaterialQuery<'a> {
    /// IDs des matériaux nommés par les règles.
    pub ids: &'a MaterialIds,
    /// Réglages des règles (`material_rules.ron`).
    pub rules: &'a MaterialRulesConfig,
    /// Position monde du point.
    #[allow(dead_code)] // pas encore lue ; servira aux futures règles
    pub pos: Vec3,
    /// Profondeur sous le relief (0 sur l'iso-surface).
    pub depth: f64,
    /// Climat à l'altitude de la surface.
    pub climate: LocalClimate,
    /// Bruit unitaire (~[−1,35 ; 1,35]) qui fait onduler les frontières ; chaque règle
    /// le met à son échelle (cf. [`noise::MaterialNoise::jitter`]).
    pub jitter: f64,
    /// Distance à la côte (voxels, > 0 terre), jitter des plages inclus. Définit les
    /// plages : la plaine côtière est trop plate pour un seuil d'altitude (1 voxel
    /// d'altitude ≈ 100 voxels de large).
    pub shore_d: f64,
    /// Profondeur sous le niveau de la mer (voxels, < 0 hors de l'eau), jitter inclus.
    pub sea_depth: f64,
    /// Normale de la surface exposée ; `None` pour les parois et le fond.
    pub normal: Option<Vec3>,
    /// Cosinus de la pente macro (moyenne sur `slope.macro_radius`) ; `None` hors
    /// surface exposée.
    pub macro_up: Option<f64>,
    /// Bruit des plaques de terre (cf. [`noise::MaterialNoise::dirt_patch`]) ; `None` hors
    /// surface exposée.
    pub patch_noise: Option<f64>,
    /// Bruit de géologie (cf. [`noise::MaterialNoise::geology`]) ; `None` hors surface
    /// exposée.
    pub geology_noise: Option<f64>,
}

/// Matériau (ou mélange) **plein** d'un point : couche de base, puis
/// modificateurs successifs. L'ordre compte : une étape tardive recouvre les
/// précédentes.
pub fn evaluate(q: &MaterialQuery) -> Voxel {
    let mut mix = rules::base_layer(q);
    rules::dirt_patches(q, &mut mix);
    rules::slope_rock(q, &mut mix);
    mix.to_voxel()
}
