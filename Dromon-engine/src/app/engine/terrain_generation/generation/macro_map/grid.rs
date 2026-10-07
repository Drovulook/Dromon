use rayon::prelude::*;

/// Géométrie de la carte macro : carré de `n × n` cellules de `cell_size` voxels,
/// couvrant le disque du monde. La cellule `(i, j)` a pour centre
/// `origin + (i + ½, j + ½) · cell_size` (`i` selon X, `j` selon Y).
#[derive(Clone, Copy, Debug)]
pub struct GridShape {
    pub n: usize,
    pub cell_size: f64,
    /// Coin bas-gauche (−X, −Y) de la grille, en coordonnées monde.
    pub origin: f64,
}

impl GridShape {
    /// Grille couvrant `[-radius, radius]²` (au moins une cellule).
    pub fn covering(radius: f64, cell_size: f64) -> GridShape {
        let n = ((2.0 * radius / cell_size).ceil() as usize).max(1);
        // Centrée sur 0 : la grille déborde du disque de la même marge des deux côtés.
        GridShape {
            n,
            cell_size,
            origin: -(n as f64) * cell_size / 2.0,
        }
    }

    /// Centre monde de la cellule `(i, j)`.
    pub fn cell_center(&self, i: usize, j: usize) -> (f64, f64) {
        (
            self.origin + (i as f64 + 0.5) * self.cell_size,
            self.origin + (j as f64 + 0.5) * self.cell_size,
        )
    }
}

/// Un champ scalaire 2D sur la carte macro (relief, température…), rangé ligne par
/// ligne : `data[j·n + i]`.
pub struct MacroGrid {
    shape: GridShape,
    data: Vec<f32>,
}

impl MacroGrid {
    /// Remplit chaque cellule avec `f(centre monde)`, en parallèle par lignes.
    pub fn from_fn(shape: GridShape, f: impl Fn(f64, f64) -> f32 + Sync) -> MacroGrid {
        let mut data = vec![0.0; shape.n * shape.n];
        data.par_chunks_mut(shape.n)
            .enumerate()
            .for_each(|(j, row)| {
                for (i, v) in row.iter_mut().enumerate() {
                    let (x, y) = shape.cell_center(i, j);
                    *v = f(x, y);
                }
            });
        MacroGrid { shape, data }
    }

    /// Grille à partir de valeurs déjà rangées ligne par ligne.
    pub fn from_vec(shape: GridShape, data: Vec<f32>) -> MacroGrid {
        assert_eq!(data.len(), shape.n * shape.n);
        MacroGrid { shape, data }
    }

    pub fn shape(&self) -> GridShape {
        self.shape
    }

    /// Toutes les valeurs, ligne par ligne (`[j·n + i]`).
    pub fn values(&self) -> &[f32] {
        &self.data
    }

    pub fn values_mut(&mut self) -> &mut [f32] {
        &mut self.data
    }

    pub fn get(&self, i: usize, j: usize) -> f32 {
        self.data[j * self.shape.n + i]
    }

    /// Valeur interpolée (bilinéaire) au point monde `(wx, wy)`. Hors grille : la
    /// cellule de bord la plus proche.
    pub fn sample(&self, wx: f64, wy: f64) -> f32 {
        // Coordonnées continues où les centres de cellule tombent sur les entiers.
        let max = (self.shape.n - 1) as f64;
        let u = ((wx - self.shape.origin) / self.shape.cell_size - 0.5).clamp(0.0, max);
        let v = ((wy - self.shape.origin) / self.shape.cell_size - 0.5).clamp(0.0, max);
        let (i0, j0) = (u as usize, v as usize);
        let (i1, j1) = ((i0 + 1).min(self.shape.n - 1), (j0 + 1).min(self.shape.n - 1));
        let (tx, ty) = ((u - i0 as f64) as f32, (v - j0 as f64) as f32);
        let bottom = self.get(i0, j0) * (1.0 - tx) + self.get(i1, j0) * tx;
        let top = self.get(i0, j1) * (1.0 - tx) + self.get(i1, j1) * tx;
        bottom * (1.0 - ty) + top * ty
    }

    /// `(min, max)` des valeurs de la grille.
    pub fn min_max(&self) -> (f32, f32) {
        self.data
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            })
    }
}
