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

    /// Toutes les cellules `(i, j)`, dans l'ordre de rangement (ligne par ligne).
    pub fn cells(&self) -> impl Iterator<Item = (usize, usize)> + use<> {
        let n = self.n;
        (0..n * n).map(move |k| (k % n, k / n))
    }

    /// Transport semi-lagrangien : point monde d'où vient ce qui arrive au centre de
    /// `(i, j)` en un pas, porté par le champ `(east, north)` (une cellule par pas à
    /// norme 1). On y lit la valeur précédente au lieu de pousser vers l'aval.
    pub fn upstream(&self, i: usize, j: usize, east: &MacroGrid, north: &MacroGrid) -> (f64, f64) {
        let (x, y) = self.cell_center(i, j);
        let (u, v) = (east.get(i, j) as f64, north.get(i, j) as f64);
        (x - u * self.cell_size, y - v * self.cell_size)
    }
}

/// Spline de Catmull-Rom entre `p[1]` (t = 0) et `p[2]` (t = 1) : cubique dont la pente
/// en chaque point vaut celle de la corde entre ses deux voisins, partagée par les
/// segments qui s'y rejoignent → pente continue. Calcul en `f64` : `d` atteint ~10⁴.
fn catmull_rom(p: [f64; 4], t: f64) -> f64 {
    let [p0, p1, p2, p3] = p;
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (3.0 * (p1 - p2) + p3 - p0) * t * t * t)
}

/// Un champ scalaire 2D sur la carte macro (relief, température…), rangé ligne par
/// ligne : `data[j·n + i]`.
pub struct MacroGrid {
    shape: GridShape,
    data: Vec<f32>,
}

impl MacroGrid {
    /// Remplit chaque cellule `(i, j)` avec `f(i, j)`, en parallèle par lignes.
    pub fn from_cells(shape: GridShape, f: impl Fn(usize, usize) -> f32 + Sync) -> MacroGrid {
        let mut data = vec![0.0; shape.n * shape.n];
        data.par_chunks_mut(shape.n).enumerate().for_each(|(j, row)| {
            for (i, v) in row.iter_mut().enumerate() {
                *v = f(i, j);
            }
        });
        MacroGrid { shape, data }
    }

    /// Remplit chaque cellule avec `f(centre monde)`, en parallèle par lignes.
    pub fn from_fn(shape: GridShape, f: impl Fn(f64, f64) -> f32 + Sync) -> MacroGrid {
        MacroGrid::from_cells(shape, |i, j| {
            let (x, y) = shape.cell_center(i, j);
            f(x, y)
        })
    }

    /// Grille à partir de valeurs déjà rangées ligne par ligne.
    pub fn from_vec(shape: GridShape, data: Vec<f32>) -> MacroGrid {
        assert_eq!(data.len(), shape.n * shape.n);
        MacroGrid { shape, data }
    }

    /// `f` appliquée à chaque valeur.
    pub fn map(&self, f: impl Fn(f32) -> f32) -> MacroGrid {
        MacroGrid {
            shape: self.shape,
            data: self.data.iter().map(|&v| f(v)).collect(),
        }
    }

    /// `f` appliquée cellule par cellule à `self` et `other` (même forme).
    pub fn zip_map(&self, other: &MacroGrid, f: impl Fn(f32, f32) -> f32) -> MacroGrid {
        assert_eq!(self.data.len(), other.data.len());
        MacroGrid {
            shape: self.shape,
            data: self.data.iter().zip(&other.data).map(|(&a, &b)| f(a, b)).collect(),
        }
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

    /// Comme [`get`](Self::get), hors grille : la cellule de bord la plus proche.
    pub fn get_clamped(&self, i: isize, j: isize) -> f32 {
        let last = self.shape.n as isize - 1;
        self.get(i.clamp(0, last) as usize, j.clamp(0, last) as usize)
    }

    /// Gradient `(∂/∂x, ∂/∂y)` en `(i, j)`, en unités du champ par voxel (différences
    /// centrées, bords répliqués).
    pub fn gradient(&self, i: usize, j: usize) -> (f32, f32) {
        let (i, j) = (i as isize, j as isize);
        let two_cells = 2.0 * self.shape.cell_size as f32;
        (
            (self.get_clamped(i + 1, j) - self.get_clamped(i - 1, j)) / two_cells,
            (self.get_clamped(i, j + 1) - self.get_clamped(i, j - 1)) / two_cells,
        )
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

    /// Valeur interpolée (bicubique, Catmull-Rom) au point monde `(wx, wy)`, sur les
    /// 4 × 4 cellules voisines. Contrairement à [`sample`](Self::sample), la **pente**
    /// est continue d'une cellule à l'autre : à préférer pour ce qui devient de la
    /// géométrie (sinon plis visibles à l'éclairage). ⚠ Peut dépasser légèrement
    /// l'intervalle des valeurs voisines.
    pub fn sample_bicubic(&self, wx: f64, wy: f64) -> f32 {
        let max = (self.shape.n - 1) as f64;
        let u = ((wx - self.shape.origin) / self.shape.cell_size - 0.5).clamp(0.0, max);
        let v = ((wy - self.shape.origin) / self.shape.cell_size - 0.5).clamp(0.0, max);
        let (i0, j0) = (u.floor() as isize, v.floor() as isize);
        let (tx, ty) = (u - i0 as f64, v - j0 as f64);
        let at = |i: isize, j: isize| self.get_clamped(i, j) as f64;
        let row = |j: isize| {
            catmull_rom([at(i0 - 1, j), at(i0, j), at(i0 + 1, j), at(i0 + 2, j)], tx)
        };
        catmull_rom([row(j0 - 1), row(j0), row(j0 + 1), row(j0 + 2)], ty) as f32
    }

    /// Valeur (bilinéaire) au centre de la cellule `(i, j)` d'une **autre** grille
    /// `shape` : rééchantillonne la grille climatique sur la grille fine.
    pub fn sample_cell(&self, shape: GridShape, i: usize, j: usize) -> f32 {
        let (x, y) = shape.cell_center(i, j);
        self.sample(x, y)
    }

    /// Grille `factor` fois plus grossière, de même origine : chaque cellule est la
    /// moyenne du bloc `factor × factor` qu'elle couvre (tronqué au bord).
    pub fn downsample(&self, factor: usize) -> MacroGrid {
        let n = self.shape.n;
        let shape = GridShape {
            n: n.div_ceil(factor),
            cell_size: self.shape.cell_size * factor as f64,
            origin: self.shape.origin,
        };
        MacroGrid::from_cells(shape, |bi, bj| {
            let (mut sum, mut count) = (0.0, 0);
            for j in bj * factor..((bj + 1) * factor).min(n) {
                for i in bi * factor..((bi + 1) * factor).min(n) {
                    sum += self.get(i, j);
                    count += 1;
                }
            }
            sum / count as f32
        })
    }

    /// Flou 3 × 3 (moyenne des voisins, bords répliqués).
    pub fn blur(&self) -> MacroGrid {
        MacroGrid::from_cells(self.shape, |i, j| {
            let (i, j) = (i as isize, j as isize);
            let mut sum = 0.0;
            for dj in -1..=1 {
                for di in -1..=1 {
                    sum += self.get_clamped(i + di, j + dj);
                }
            }
            sum / 9.0
        })
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
