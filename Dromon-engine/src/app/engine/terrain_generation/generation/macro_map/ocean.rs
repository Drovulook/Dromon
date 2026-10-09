//! **Courants marins et température de surface de la mer (SST)**, sur la grille
//! climatique. Pas de simulation d'océan : les courants suivent le vent, déviés le
//! long des côtes, et transportent la température de l'eau. Ils ne forment pas les
//! grandes boucles (gyres) des vrais océans : seulement « l'eau se souvient d'où elle
//! vient » (côte longée par un courant venu du nord → eau froide, côte sèche).

use super::grid::MacroGrid;
use crate::app::engine::terrain_generation::utils::smoothstep;
use crate::config::ClimateParams;

/// Courants marins et remontée d'eau froide.
pub struct Currents {
    /// Courant `(est, nord)`, en multiples de la vitesse nominale du vent ; nul sur terre.
    pub east: MacroGrid,
    pub north: MacroGrid,
    /// Intensité de la remontée d'eau froide (≥ 0, ~1 à vent nominal contre la côte).
    pub upwelling: MacroGrid,
}

/// Près des côtes, le vent est décomposé selon `n` (direction de la terre la plus
/// proche) :
/// - sa part **vers la terre** est retirée du courant : l'eau longe la côte au lieu
///   d'y entrer ;
/// - sa part **vers le large** pousse l'eau de surface loin de la côte : de l'eau
///   froide des profondeurs remonte la remplacer (upwelling).
///
/// `coast` = distance signée à la côte (voxels, > 0 terre).
pub fn currents(
    params: &ClimateParams,
    wind_east: &MacroGrid,
    wind_north: &MacroGrid,
    coast: &MacroGrid,
) -> Currents {
    let shape = coast.shape();
    let cells = shape.n * shape.n;
    let mut east = Vec::with_capacity(cells);
    let mut north = Vec::with_capacity(cells);
    let mut upwelling = Vec::with_capacity(cells);
    for (i, j) in shape.cells() {
        let (mut u, mut v) = (wind_east.get(i, j), wind_north.get(i, j));
        let mut up = 0.0;
        let d = coast.get(i, j);
        if d > 0.0 {
            // Terre : pas de courant.
            (u, v) = (0.0, 0.0);
        } else {
            // `d` croît vers la terre : son gradient pointe vers la côte la plus proche.
            let (gx, gy) = coast.gradient(i, j);
            let len = (gx * gx + gy * gy).sqrt();
            if len > 0.0 {
                let (nx, ny) = (gx / len, gy / len);
                let along_n = u * nx + v * ny; // > 0 vers la terre, < 0 vers le large
                let near = 1.0 - smoothstep(0.0, params.current_coast_width, -d as f64) as f32;
                let toward_land = along_n.max(0.0);
                (u, v) = (u - near * toward_land * nx, v - near * toward_land * ny);
                up = near * (-along_n).max(0.0);
            }
        }
        east.push(u);
        north.push(v);
        upwelling.push(up);
    }
    Currents {
        east: MacroGrid::from_vec(shape, east),
        north: MacroGrid::from_vec(shape, north),
        upwelling: MacroGrid::from_vec(shape, upwelling),
    }
}

/// SST (°C) : à chaque itération, l'eau arrive d'une cellule en amont du courant
/// (transport semi-lagrangien : on lit `sst(position − courant · pas)`), puis se
/// rapproche de l'équilibre de sa latitude. Une anomalie s'efface sur
/// `current_reach` voxels de trajet.
///
/// Upwelling : l'eau remontée **remplace** l'eau de surface, elle est froide
/// d'emblée (`min`, pas un rééquilibrage lent que le courant emporterait avant
/// effet). Elle s'étire ensuite en langue froide vers l'aval.
pub fn sea_surface_temperature(
    params: &ClimateParams,
    equilibrium: &MacroGrid,
    currents: &Currents,
) -> MacroGrid {
    let shape = equilibrium.shape();
    let strength = params.upwelling_strength as f32;
    let upwelled = |i: usize, j: usize, t: f32| {
        let up = currents.upwelling.get(i, j);
        if up > 0.0 {
            t.min(equilibrium.get(i, j) - strength * up)
        } else {
            t
        }
    };
    if params.current_reach <= 0.0 {
        return MacroGrid::from_cells(shape, |i, j| upwelled(i, j, equilibrium.get(i, j)));
    }
    // Pas de temps : un courant nominal avance d'une cellule par itération.
    let step = shape.cell_size;
    let relax = (step / params.current_reach).min(1.0) as f32;
    // ~4 portées de trajet : l'anomalie initiale (nulle) est oubliée à e⁻⁴ ≈ 2 %.
    let iterations = ((4.0 * params.current_reach / step).ceil() as usize).max(1);

    let mut sst = equilibrium.map(|t| t);
    for _ in 0..iterations {
        sst = MacroGrid::from_cells(shape, |i, j| {
            let (ux, uy) = shape.upstream(i, j, &currents.east, &currents.north);
            let arrived = sst.sample(ux, uy);
            let eq = equilibrium.get(i, j);
            upwelled(i, j, arrived + (eq - arrived) * relax)
        });
    }
    sst
}
