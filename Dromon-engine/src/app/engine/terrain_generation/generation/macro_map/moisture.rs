//! **Humidité** : transport de la vapeur d'eau par le vent, sur la grille climatique.
//!
//! La vapeur `q` (1 = air saturé à 20 °C) se charge au-dessus de la mer, voyage avec
//! le vent et tombe en pluie au-dessus des terres. Les effets non locaux en
//! découlent sans être codés : intérieur des continents sec (la vapeur s'épuise en
//! route), versant au vent arrosé et ombre pluviométrique derrière (l'air qui monte
//! pleut), côtes sèches face à une mer froide (évaporation faible), bande calme sèche
//! (aucun air n'y arrive de l'océan).
//!
//! Chaque itération, pour chaque cellule :
//! 1. **transport** semi-lagrangien : l'air vient de `position − vent · pas` ;
//! 2. **mer** : recharge vers la saturation à la SST ;
//!    **terre** : pluie de base + pluie orographique (montée de l'air) ;
//! 3. **saturation** à la température de l'air : l'excédent tombe ;
//! 4. légère **diffusion** (brises : humidité côtière même sans vent).
//!
//! Résultat : le débit de pluie une fois l'état stable atteint.

use rayon::prelude::*;

use super::grid::MacroGrid;
use crate::config::ClimateParams;

/// Vapeur maximale que l'air peut contenir à `t` °C, 1 à 20 °C : +7 % par °C
/// (Clausius-Clapeyron). ~0,25 à 0 °C : l'air refroidi en montagne doit pleuvoir.
fn saturation(t: f32) -> f32 {
    (0.07 * (t - 20.0)).exp()
}

/// Entrées de [`precipitation`], toutes sur la grille climatique.
pub struct MoistureInputs<'a> {
    pub wind_east: &'a MacroGrid,
    pub wind_north: &'a MacroGrid,
    /// Température de l'air (°C), à l'altitude de la cellule.
    pub temperature: &'a MacroGrid,
    /// Température de surface de la mer (°C).
    pub sst: &'a MacroGrid,
    /// Altitude (voxels) et distance signée à la côte (> 0 terre).
    pub relief: &'a MacroGrid,
    pub coast: &'a MacroGrid,
}

/// Précipitations à l'état stable, en multiples de la pluie d'une plaine côtière
/// recevant un air saturé à 20 °C : 0 = désert, 1 = côte humide, > 1 = versant
/// exposé. Indépendant des réglages : utilisable tel quel pour les biomes.
pub fn precipitation(params: &ClimateParams, input: &MoistureInputs) -> MacroGrid {
    let shape = input.relief.shape();
    let n = shape.n;
    let evaporation = params.evaporation as f32;
    let rain_base = params.rain_base as f32;
    let rain_oro = params.rain_orographic as f32 / 100.0;
    let diffusion = params.moisture_diffusion as f32;
    // L'air doit pouvoir traverser le monde : ~1 cellule par itération à vent nominal.
    let iterations = 2 * n;

    let mut q = MacroGrid::from_vec(shape, vec![0.0; n * n]);
    let mut rain = vec![0.0f32; n * n];
    for _ in 0..iterations {
        let mut next = vec![0.0f32; n * n];
        next.par_chunks_mut(n)
            .zip(rain.par_chunks_mut(n))
            .enumerate()
            .for_each(|(j, (q_row, rain_row))| {
                for i in 0..n {
                    // Rempli à la fois `q` et `rain` : pas de `from_cells` ici.
                    let (ux, uy) = shape.upstream(i, j, input.wind_east, input.wind_north);
                    let mut vapor = q.sample(ux, uy);
                    let mut fallen = 0.0;
                    if input.coast.get(i, j) <= 0.0 {
                        let deficit = saturation(input.sst.get(i, j)) - vapor;
                        vapor += evaporation * deficit.max(0.0);
                    } else {
                        let rise = (input.relief.get(i, j) - input.relief.sample(ux, uy)).max(0.0);
                        fallen = vapor * (rain_base + rain_oro * rise).min(1.0);
                        vapor -= fallen;
                    }
                    let cap = saturation(input.temperature.get(i, j));
                    if vapor > cap {
                        fallen += vapor - cap;
                        vapor = cap;
                    }
                    q_row[i] = vapor;
                    rain_row[i] = fallen;
                }
            });
        q = diffuse(MacroGrid::from_vec(shape, next), diffusion);
    }

    // Normalisation : plaine côtière, air saturé à 20 °C → `rain_base`.
    MacroGrid::from_vec(shape, rain).map(|r| r / rain_base).blur().blur()
}

/// Mélange chaque cellule avec la moyenne de ses 4 voisines (fraction `amount`).
fn diffuse(q: MacroGrid, amount: f32) -> MacroGrid {
    if amount <= 0.0 {
        return q;
    }
    MacroGrid::from_cells(q.shape(), |i, j| {
        let (i, j) = (i as isize, j as isize);
        let at = |di, dj| q.get_clamped(i + di, j + dj);
        let mean = (at(-1, 0) + at(1, 0) + at(0, -1) + at(0, 1)) / 4.0;
        let here = at(0, 0);
        here + (mean - here) * amount
    })
}
