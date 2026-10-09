//! Exports des terres : continents, distance à la côte, montagnes, relief.

use std::path::Path;

use anyhow::Result;
use image::{Rgb, RgbImage};

use super::palette::{LAND_HIGH, LAND_LOW, SEA_DEEP, SEA_SHALLOW, lerp, ramp, to_rgb};
use super::pixels::{boundary, isoline, render, save};
use crate::app::engine::terrain_generation::generation::macro_map::grid::MacroGrid;

/// Champ signé (< 0 mer, > 0 terre) : bleu de plus en plus foncé en s'éloignant de 0
/// côté mer, vert → brun côté terre, saturés à `±range`. Une isoligne tous les `iso`
/// (unités du champ), épaisse d'un pixel quelle que soit la pente ; le niveau 0 (la
/// côte) en noir. Isolignes régulières ⇔ le champ varie comme une distance.
pub fn save_signed(grid: &MacroGrid, path: &Path, range: f32, iso: f32) -> Result<()> {
    let n = grid.shape().n;
    let img = render(n, |i, j| {
        if boundary(n, i, j, |i, j| grid.get(i, j) > 0.0) {
            return Rgb([0, 0, 0]);
        }
        let v = grid.get(i, j);
        let t = (v.abs() / range).min(1.0);
        let (a, z) = if v < 0.0 {
            (SEA_SHALLOW, SEA_DEEP)
        } else {
            (LAND_LOW, LAND_HIGH)
        };
        let shade = if isoline(n, i, j, |i, j| grid.get(i, j), iso) { 0.65 } else { 1.0 };
        Rgb(std::array::from_fn(|c| ((a[c] + (z[c] - a[c]) * t) * shade) as u8))
    });
    save(&img, path)
}

/// Masque `[0, 1]` sur les terres (`coast > 0`) : vert → brun, isoligne tous les
/// 0,25 ; mer unie, côte en noir.
pub fn save_mask(mask: &MacroGrid, coast: &MacroGrid, path: &Path) -> Result<()> {
    const SEA: [f32; 3] = [25.0, 45.0, 100.0];
    const FULL: [f32; 3] = [125.0, 65.0, 40.0];
    let n = mask.shape().n;
    let is_land = |i, j| coast.get(i, j) > 0.0;
    let img = render(n, |i, j| {
        if boundary(n, i, j, is_land) {
            return Rgb([0, 0, 0]);
        }
        if !is_land(i, j) {
            return to_rgb(SEA, 1.0);
        }
        let iso = isoline(n, i, j, |i, j| mask.get(i, j), 0.25);
        to_rgb(lerp(LAND_LOW, FULL, mask.get(i, j)), if iso { 0.65 } else { 1.0 })
    });
    save(&img, path)
}

/// Teintes hypsométriques (bleus sous `sea_level`, vert → brun → blanc jusqu'à
/// `max_height`) + **ombrage** (lumière du nord-ouest) pour lire les pentes. Côte en
/// noir.
pub fn save_relief(h: &MacroGrid, path: &Path, sea_level: f32, max_height: f32) -> Result<()> {
    save(&relief_image(h, sea_level, max_height), path)
}

/// Image de [`save_relief`], réutilisable comme fond (cf. `save_arrows`).
pub fn relief_image(h: &MacroGrid, sea_level: f32, max_height: f32) -> RgbImage {
    // Altitude relative (0 = mer, 1 = max_height) → couleur, par segments.
    const LAND: [(f32, [f32; 3]); 4] = [
        (0.0, LAND_LOW),
        (0.3, [185.0, 175.0, 105.0]),
        (0.6, LAND_HIGH),
        (0.9, [240.0, 240.0, 240.0]),
    ];
    let n = h.shape().n;
    render(n, |i, j| {
        if boundary(n, i, j, |i, j| h.get(i, j) < sea_level) {
            return Rgb([0, 0, 0]);
        }
        let v = h.get(i, j);
        let color = if v < sea_level {
            lerp(SEA_SHALLOW, SEA_DEEP, ((sea_level - v) / sea_level.max(1.0)).min(1.0))
        } else {
            ramp(&LAND, (v - sea_level) / (max_height - sea_level).max(1.0))
        };
        to_rgb(color, hillshade(h, i, j))
    })
}

/// Ombrage du relief `h` en `(i, j)`, lumière du nord-ouest : 1 sur sol plat, < 1
/// sur les versants à l'ombre, > 1 face à la lumière.
pub(super) fn hillshade(h: &MacroGrid, i: usize, j: usize) -> f32 {
    // Pentes macro douces (≲ 15°) : exagérées pour que l'ombrage se lise.
    const EXAGGERATION: f32 = 3.0;
    const LIGHT: [f32; 3] = [-1.0, 1.0, 1.4];
    let len = (LIGHT[0] * LIGHT[0] + LIGHT[1] * LIGHT[1] + LIGHT[2] * LIGHT[2]).sqrt();
    let light = LIGHT.map(|v| v / len);

    let (gx, gy) = h.gradient(i, j);
    let (gx, gy) = (gx * EXAGGERATION, gy * EXAGGERATION);
    let dot = (-gx * light[0] - gy * light[1] + light[2]) / (gx * gx + gy * gy + 1.0).sqrt();
    (dot / light[2]).clamp(0.3, 1.3)
}
