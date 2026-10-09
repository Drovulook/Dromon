//! Exports du climat : température de l'air, SST et son anomalie, précipitations.
//!
//! Les champs climatiques (grille grossière) sont rééchantillonnés en bilinéaire à la
//! résolution de la carte fine `coast` — ce que verra le moteur.

use std::path::Path;

use anyhow::Result;
use image::Rgb;

use super::land::hillshade;
use super::palette::{LAND_GREY, ramp, to_rgb};
use super::pixels::{boundary, isoline, render, save};
use crate::app::engine::terrain_generation::generation::macro_map::grid::MacroGrid;

/// Température (°C). Isotherme tous les 5 °C (0 °C plus marqué), côte en noir.
/// `sea_only` : terres en gris (SST), sinon mer assombrie.
pub fn save_temperature(
    t: &MacroGrid,
    coast: &MacroGrid,
    sea_only: bool,
    path: &Path,
) -> Result<()> {
    const STOPS: [(f32, [f32; 3]); 7] = [
        (-20.0, [50.0, 60.0, 160.0]),
        (-10.0, [90.0, 140.0, 220.0]),
        (0.0, [230.0, 235.0, 240.0]),
        (10.0, [250.0, 220.0, 120.0]),
        (20.0, [245.0, 150.0, 60.0]),
        (30.0, [200.0, 50.0, 40.0]),
        (40.0, [120.0, 20.0, 30.0]),
    ];
    let shape = coast.shape();
    let n = shape.n;
    let temp = |i, j| t.sample_cell(shape, i, j);
    let is_land = |i, j| coast.get(i, j) > 0.0;
    let img = render(n, |i, j| {
        if boundary(n, i, j, is_land) {
            return Rgb([0, 0, 0]);
        }
        if sea_only && is_land(i, j) {
            return to_rgb(LAND_GREY, 1.0);
        }
        let shade = if boundary(n, i, j, |i, j| temp(i, j) < 0.0) {
            0.35
        } else if isoline(n, i, j, temp, 5.0) {
            0.7
        } else {
            1.0
        };
        let sea = if is_land(i, j) || sea_only { 1.0 } else { 0.75 };
        to_rgb(ramp(&STOPS, temp(i, j)), shade * sea)
    });
    save(&img, path)
}

/// Précipitations sur les terres : beige (désert) → vert → vert sombre → bleu (> 1,5,
/// versants exposés), isoligne tous les 0,25 ; ombrage du relief pour situer les
/// chaînes ; mer unie, côte en noir.
pub fn save_precipitation(
    p: &MacroGrid,
    relief: &MacroGrid,
    coast: &MacroGrid,
    path: &Path,
) -> Result<()> {
    const STOPS: [(f32, [f32; 3]); 5] = [
        (0.0, [215.0, 190.0, 130.0]),
        (0.25, [225.0, 215.0, 140.0]),
        (0.5, [140.0, 185.0, 95.0]),
        (1.0, [45.0, 125.0, 65.0]),
        (2.0, [40.0, 90.0, 170.0]),
    ];
    const SEA: [f32; 3] = [25.0, 45.0, 100.0];
    let shape = coast.shape();
    let n = shape.n;
    let rain = |i, j| p.sample_cell(shape, i, j);
    let is_land = |i, j| coast.get(i, j) > 0.0;
    let img = render(n, |i, j| {
        if boundary(n, i, j, is_land) {
            return Rgb([0, 0, 0]);
        }
        if !is_land(i, j) {
            return to_rgb(SEA, 1.0);
        }
        let iso = isoline(n, i, j, rain, 0.25);
        let shade = hillshade(relief, i, j) * if iso { 0.75 } else { 1.0 };
        to_rgb(ramp(&STOPS, rain(i, j)), shade)
    });
    save(&img, path)
}

/// Anomalie de SST (`sst − équilibre`, °C) : bleu = courant froid, rouge = courant
/// chaud, saturés à ±`range` ; isoligne tous les 1 °C ; terres en gris.
pub fn save_sst_anomaly(
    sst: &MacroGrid,
    equilibrium: &MacroGrid,
    coast: &MacroGrid,
    range: f32,
    path: &Path,
) -> Result<()> {
    let stops = [
        (-range, [40.0, 80.0, 200.0]),
        (0.0, [240.0, 240.0, 240.0]),
        (range, [210.0, 50.0, 40.0]),
    ];
    let shape = coast.shape();
    let n = shape.n;
    let anomaly = |i, j| sst.sample_cell(shape, i, j) - equilibrium.sample_cell(shape, i, j);
    let is_land = |i, j| coast.get(i, j) > 0.0;
    let img = render(n, |i, j| {
        if boundary(n, i, j, is_land) {
            return Rgb([0, 0, 0]);
        }
        if is_land(i, j) {
            return to_rgb(LAND_GREY, 1.0);
        }
        let iso = isoline(n, i, j, anomaly, 1.0);
        to_rgb(ramp(&stops, anomaly(i, j)), if iso { 0.7 } else { 1.0 })
    });
    save(&img, path)
}
