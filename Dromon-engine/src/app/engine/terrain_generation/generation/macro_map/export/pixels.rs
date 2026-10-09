//! Outils communs aux exports : rendu pixel par pixel et lignes de contour.
//!
//! Une image a autant de pixels que la grille a de cellules : le pixel `(px, py)`
//! est la cellule `(i, j) = (px, n − 1 − py)` (nord en haut).

use std::path::Path;

use anyhow::{Context, Result};
use image::{Rgb, RgbImage};

/// Image `n × n` dont chaque pixel vaut `pixel(i, j)` pour sa cellule.
pub fn render(n: usize, pixel: impl Fn(usize, usize) -> Rgb<u8>) -> RgbImage {
    RgbImage::from_fn(n as u32, n as u32, |px, py| pixel(px as usize, n - 1 - py as usize))
}

pub fn save(img: &RgbImage, path: &Path) -> Result<()> {
    img.save(path)
        .with_context(|| format!("écriture de {}", path.display()))
}

/// La cellule `(i, j)` est-elle sur une frontière de `side` : son voisin de droite
/// ou du dessus est-il de l'autre côté ? Donne un trait d'un pixel (côte, 0 °C…).
pub fn boundary(n: usize, i: usize, j: usize, side: impl Fn(usize, usize) -> bool) -> bool {
    let here = side(i, j);
    (i + 1 < n && side(i + 1, j) != here) || (j + 1 < n && side(i, j + 1) != here)
}

/// La cellule `(i, j)` est-elle sur une isoligne de `value`, tracée tous les `step` ?
/// Même principe que [`boundary`] sur les bandes `⌊value / step⌋` : un pixel
/// d'épaisseur quelle que soit la pente du champ.
pub fn isoline(n: usize, i: usize, j: usize, value: impl Fn(usize, usize) -> f32, step: f32) -> bool {
    let band = |i, j| (value(i, j) / step).floor();
    let here = band(i, j);
    (i + 1 < n && band(i + 1, j) != here) || (j + 1 < n && band(i, j + 1) != here)
}
