//! Export des champs de vecteurs (vent, courants) en flèches.

use std::path::Path;

use anyhow::Result;
use image::{Rgb, RgbImage};

use super::pixels::save;
use crate::app::engine::terrain_generation::generation::macro_map::grid::{GridShape, MacroGrid};

/// Flèches d'un champ de vecteurs (vent, courants) sur `background` (image à la
/// résolution de `shape`) : une tous les `STEP` pixels, de longueur proportionnelle à
/// la norme. `sea_only` (distance à la côte) : flèches en mer seulement.
pub fn save_arrows(
    mut background: RgbImage,
    east: &MacroGrid,
    north: &MacroGrid,
    shape: GridShape,
    sea_only: Option<&MacroGrid>,
    color: Rgb<u8>,
    path: &Path,
) -> Result<()> {
    const STEP: usize = 80;
    const NOMINAL_LEN: f32 = 50.0; // longueur (px) d'une flèche de norme 1
    const HEAD: f32 = 12.0;
    const MIN_LEN: f32 = 0.1; // norme sous laquelle on ne dessine rien
    let n = shape.n;
    for j in (STEP / 2..n).step_by(STEP) {
        for i in (STEP / 2..n).step_by(STEP) {
            if sea_only.is_some_and(|coast| coast.get(i, j) > 0.0) {
                continue;
            }
            let (x, y) = shape.cell_center(i, j);
            if east.sample(x, y).hypot(north.sample(x, y)) < MIN_LEN {
                continue;
            }
            // Image : y vers le bas, d'où le signe de la composante nord.
            let (dx, dy) = (east.sample(x, y), -north.sample(x, y));
            let (x0, y0) = (i as f32, (n - 1 - j) as f32);
            let (x1, y1) = (x0 + dx * NOMINAL_LEN, y0 + dy * NOMINAL_LEN);
            draw_line(&mut background, (x0, y0), (x1, y1), color);
            // Pointe : deux traits à ±150° de la direction.
            let angle = dy.atan2(dx);
            for side in [-1.0f32, 1.0] {
                let a = angle + side * 150f32.to_radians();
                draw_line(&mut background, (x1, y1), (x1 + HEAD * a.cos(), y1 + HEAD * a.sin()), color);
            }
        }
    }
    save(&background, path)
}

/// Segment épais de 2 px, tracé point par point (pas assez de besoins pour une crate
/// de dessin).
fn draw_line(img: &mut RgbImage, (x0, y0): (f32, f32), (x1, y1): (f32, f32), color: Rgb<u8>) {
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil().max(1.0) as usize;
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let (x, y) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
        for (ox, oy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let (px, py) = ((x + ox) as i64, (y + oy) as i64);
            if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                img.put_pixel(px as u32, py as u32, color);
            }
        }
    }
}
