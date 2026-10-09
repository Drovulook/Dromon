//! Masque des **chaînes de montagnes** `w ∈ [0, 1]` : 1 sur l'axe d'une chaîne, 0 en
//! plaine. Le détail mélangera les hauteurs `w · h_montagne + (1 − w) · h_plaine`.
//!
//! Axes : crêtes d'un bruit ridged (`1 − |n|` ≈ 1 le long des lignes où `n` s'annule)
//! → des lignes, pas des taches. Largeur : distance transform aux axes, donc un
//! piémont de largeur fixée en voxels — comme la côte, et pour la même raison : la
//! valeur du bruit n'est pas une distance.

use noise::{NoiseFn, SuperSimplex};

use super::distance::distance_to;
use super::grid::{GridShape, MacroGrid};
use crate::app::engine::terrain_generation::utils::smoothstep;
use crate::config::MountainParams;

// Décalages (espace bruit) : décorrélés des continents, qui partagent la graine.
const AXIS_OFF: f64 = 3517.9;
const WARP_OFF_X: f64 = 4129.3;
const WARP_OFF_Y: f64 = 4787.1;

/// Calcule le masque `w` sur la grille, à partir de la distance signée à la côte.
pub fn mountain_mask(
    seed: u32,
    params: &MountainParams,
    shape: GridShape,
    coast_distance: &MacroGrid,
) -> MacroGrid {
    let noise = SuperSimplex::new(seed);
    let ridge = MacroGrid::from_fn(shape, |x, y| ridge(&noise, params, x, y) as f32);

    // Axes : crête du bruit, assez loin de la côte.
    let axis: Vec<bool> = ridge
        .values()
        .iter()
        .zip(coast_distance.values())
        .map(|(&r, &d)| r as f64 > params.axis_threshold && d as f64 > params.min_coast_distance)
        .collect();

    // Piémont mesuré par distance à l'axe, pas par la valeur du bruit : celle-ci décroît
    // plus ou moins vite selon la pente locale du bruit (mur ici, pente de 7° là-bas).
    // La distance donne `foothill_width` partout, donc une pente de flanc maîtrisée.
    let cell = shape.cell_size;
    MacroGrid::from_vec(shape, distance_to(&axis, shape.n)).zip_map(coast_distance, |dist, d| {
        let foothill = 1.0 - smoothstep(0.0, params.foothill_width, dist as f64 * cell);
        (foothill * smoothstep(0.0, params.coast_fade, d as f64)) as f32
    })
}

/// `1 − |n|` du bruit d'axes, évalué en `p + warp(p)` (chaînes pliées).
fn ridge(noise: &SuperSimplex, p: &MountainParams, x: f64, y: f64) -> f64 {
    let (mut x, mut y) = (x, y);
    if p.warp_amplitude > 0.0 {
        let f = p.warp_frequency;
        let dx = noise.get([x * f + WARP_OFF_X, y * f + WARP_OFF_X]);
        let dy = noise.get([x * f + WARP_OFF_Y, y * f + WARP_OFF_Y]);
        (x, y) = (x + dx * p.warp_amplitude, y + dy * p.warp_amplitude);
    }
    let f = p.frequency;
    1.0 - noise.get([x * f + AXIS_OFF, y * f + AXIS_OFF]).abs()
}
