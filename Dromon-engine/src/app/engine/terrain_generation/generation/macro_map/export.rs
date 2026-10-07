//! Export PNG des champs de la carte macro, pour régler la génération sans lancer
//! le moteur. Nord (+Y) en haut : les lignes de l'image parcourent `j` à l'envers.

use std::path::Path;

use anyhow::{Context, Result};
use image::{Rgb, RgbImage};

use super::grid::MacroGrid;

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
}

fn to_rgb(c: [f32; 3], shade: f32) -> Rgb<u8> {
    Rgb(c.map(|v| (v * shade).clamp(0.0, 255.0) as u8))
}

/// Masque `[0, 1]` sur les terres (`coast > 0`) : vert → brun, isoligne tous les
/// 0,25 ; mer unie, côte en noir.
pub fn save_mask(mask: &MacroGrid, coast: &MacroGrid, path: &Path) -> Result<()> {
    const SEA: [f32; 3] = [25.0, 45.0, 100.0];
    const FULL: [f32; 3] = [125.0, 65.0, 40.0];
    let n = mask.shape().n;
    let band = |i: usize, j: usize| (mask.get(i, j) * 4.0).floor();
    let is_land = |i: usize, j: usize| coast.get(i, j) > 0.0;
    let img = RgbImage::from_fn(n as u32, n as u32, |px, py| {
        let (i, j) = (px as usize, n - 1 - py as usize);
        let right = i + 1 < n;
        let up = j + 1 < n;
        if (right && is_land(i + 1, j) != is_land(i, j)) || (up && is_land(i, j + 1) != is_land(i, j)) {
            return Rgb([0, 0, 0]);
        }
        if !is_land(i, j) {
            return to_rgb(SEA, 1.0);
        }
        let iso = (right && band(i + 1, j) != band(i, j)) || (up && band(i, j + 1) != band(i, j));
        to_rgb(lerp(LAND_LOW, FULL, mask.get(i, j)), if iso { 0.65 } else { 1.0 })
    });
    img.save(path)
        .with_context(|| format!("écriture de {}", path.display()))
}

/// Teintes hypsométriques (bleus sous `sea_level`, vert → brun → blanc jusqu'à
/// `max_height`) + **ombrage** (lumière du nord-ouest) pour lire les pentes. Côte en
/// noir.
pub fn save_relief(h: &MacroGrid, path: &Path, sea_level: f32, max_height: f32) -> Result<()> {
    // Altitude relative (0 = mer, 1 = max_height) → couleur, par segments.
    const LAND: [(f32, [f32; 3]); 4] = [
        (0.0, LAND_LOW),
        (0.3, [185.0, 175.0, 105.0]),
        (0.6, LAND_HIGH),
        (0.9, [240.0, 240.0, 240.0]),
    ];
    // Pentes macro douces (≲ 15°) : exagérées pour que l'ombrage se lise.
    const EXAGGERATION: f32 = 3.0;
    let light = {
        let l = [-1.0f32, 1.0, 1.4];
        let len = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        l.map(|v| v / len)
    };

    let shape = h.shape();
    let n = shape.n;
    let cell = shape.cell_size as f32;
    let at = |i: isize, j: isize| {
        let c = |v: isize| v.clamp(0, n as isize - 1) as usize;
        h.get(c(i), c(j))
    };
    let img = RgbImage::from_fn(n as u32, n as u32, |px, py| {
        let (i, j) = (px as usize, n - 1 - py as usize);
        let v = h.get(i, j);
        let wet = v < sea_level;
        let right = i + 1 < n && (h.get(i + 1, j) < sea_level) != wet;
        let up = j + 1 < n && (h.get(i, j + 1) < sea_level) != wet;
        if right || up {
            return Rgb([0, 0, 0]);
        }

        let color = if wet {
            lerp(SEA_SHALLOW, SEA_DEEP, ((sea_level - v) / sea_level.max(1.0)).min(1.0))
        } else {
            let t = (v - sea_level) / (max_height - sea_level).max(1.0);
            let k = LAND.windows(2).position(|w| t < w[1].0).unwrap_or(LAND.len() - 2);
            let ((t0, c0), (t1, c1)) = (LAND[k], LAND[k + 1]);
            lerp(c0, c1, ((t - t0) / (t1 - t0)).clamp(0.0, 1.0))
        };

        // Normale par différences centrées ; ombrage normalisé (sol plat = 1).
        let (si, sj) = (i as isize, j as isize);
        let gx = (at(si + 1, sj) - at(si - 1, sj)) / (2.0 * cell) * EXAGGERATION;
        let gy = (at(si, sj + 1) - at(si, sj - 1)) / (2.0 * cell) * EXAGGERATION;
        let len = (gx * gx + gy * gy + 1.0).sqrt();
        let dot = (-gx * light[0] - gy * light[1] + light[2]) / len;
        to_rgb(color, (dot / light[2]).clamp(0.3, 1.3))
    });
    img.save(path)
        .with_context(|| format!("écriture de {}", path.display()))
}

const SEA_SHALLOW: [f32; 3] = [120.0, 175.0, 225.0];
const SEA_DEEP: [f32; 3] = [10.0, 30.0, 90.0];
const LAND_LOW: [f32; 3] = [95.0, 165.0, 75.0];
const LAND_HIGH: [f32; 3] = [160.0, 125.0, 85.0];

/// Champ signé (< 0 mer, > 0 terre) : bleu de plus en plus foncé en s'éloignant de 0
/// côté mer, vert → brun côté terre, saturés à `±range`. Une isoligne tous les `iso`
/// (unités du champ), épaisse d'un pixel quelle que soit la pente ; le niveau 0 (la
/// côte) en noir. Isolignes régulières ⇔ le champ varie comme une distance.
pub fn save_signed(grid: &MacroGrid, path: &Path, range: f32, iso: f32) -> Result<()> {
    let n = grid.shape().n;
    let band = |i: usize, j: usize| (grid.get(i, j) / iso).floor();
    let img = RgbImage::from_fn(n as u32, n as u32, |px, py| {
        let (i, j) = (px as usize, n - 1 - py as usize);
        let v = grid.get(i, j);
        let b = band(i, j);
        // Frontière de bande avec le voisin de droite ou du dessus → isoligne.
        let right = i + 1 < n && band(i + 1, j) != b;
        let up = j + 1 < n && band(i, j + 1) != b;
        let on_coast = (right && (v > 0.0) != (grid.get(i + 1, j) > 0.0))
            || (up && (v > 0.0) != (grid.get(i, j + 1) > 0.0));
        if on_coast {
            return Rgb([0, 0, 0]);
        }
        let t = (v.abs() / range).min(1.0);
        let (a, z) = if v < 0.0 {
            (SEA_SHALLOW, SEA_DEEP)
        } else {
            (LAND_LOW, LAND_HIGH)
        };
        let shade = if right || up { 0.65 } else { 1.0 };
        Rgb(std::array::from_fn(|c| ((a[c] + (z[c] - a[c]) * t) * shade) as u8))
    });
    img.save(path)
        .with_context(|| format!("écriture de {}", path.display()))
}
