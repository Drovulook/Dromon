//! **Grille plate des niveaux de LOD** : topologie du monde chargé + niveau de chaque
//! chunk + masque de coutures. C'est la *configuration* que lit le mailleur.
//!
//! ## Pourquoi séparée du [`ChunkManager`](super::super::ChunkManager)
//! Le LOD vivait autrefois sur le `Chunk`, donc dans le manager. Le re-maillage de fond
//! l'interdit : un thread worker doit voir une configuration **figée** pendant tout son
//! lot, alors que la caméra continue d'en produire de nouvelles. En sortant le LOD, on
//! obtient deux objets aux durées de vie opposées — un `ChunkManager` immuable partagé
//! par tous (le relief, qui ne bouge pas) et une `LodGrid` bon marché à cloner (~40 Ko),
//! dont chaque lot emporte sa version dans un `Arc`.
//!
//! ## Grille plate plutôt que `HashMap`
//! Le LOD se recalcule sur les ~8000 chunks à chaque fois que la caméra franchit le
//! seuil, relaxation 2:1 comprise (plusieurs passes × 4 voisins). En indexation directe
//! (`(y - min_y) · largeur + (x - min_x)`) chaque voisin coûte un accès tableau au lieu
//! d'un hachage : de l'ordre de la milliseconde gagnée par passe, dans la frame.
//!
//! ## Deux niveaux par chunk : `raw` et `lod`
//! `raw` est le niveau **brut** issu de la distance seule ; c'est lui, et lui seul, que
//! relit l'hystérésis. `lod` est le niveau **après équilibrage 2:1**, ce que maille le
//! chunk. Confondre les deux fait dériver l'hystérésis (cf. `LOD-Dynamique.md`, piège 2) :
//! un chunk que la relaxation abaisse verrait sa bande morte glisser à chaque passe et ne
//! remonterait jamais. L'équilibrage se réapplique **à neuf** depuis `raw` à chaque passe.

use super::{Face, MAX_LOD_STEP, TransitionFaces, transition_faces};
use glam::IVec2;

/// État LOD d'un chunk chargé. `Copy` : la grille se manipule par valeur.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct Cell {
    /// Niveau brut (distance seule + hystérésis). Source de vérité de l'hystérésis.
    raw: u8,
    /// Niveau effectif après équilibrage 2:1. Ce que lit le mailleur.
    lod: u8,
    /// Faces bordant un voisin plus grossier (dérivé de `lod` des 4 voisins).
    faces: TransitionFaces,
}

/// Niveaux de LOD de tous les chunks chargés, sur une grille rectangulaire dont les
/// cases hors monde valent `None`. Porte donc aussi la **topologie** : « chargé » se lit
/// ici, et nulle part ailleurs (le mailleur en déduit les bords du monde).
#[derive(Clone)]
pub struct LodGrid {
    /// Coin bas-gauche (coordonnées chunk) de la boîte englobante.
    min: IVec2,
    /// Dimensions de la boîte englobante, en chunks.
    size: IVec2,
    /// `size.x · size.y` cases, en ordre ligne par ligne. `None` = hors monde.
    cells: Vec<Option<Cell>>,
    /// Les coordonnées chargées, pour itérer sans balayer les trous de la boîte.
    coords: Vec<IVec2>,
}

impl LodGrid {
    /// Grille couvrant exactement `coords`, tous les chunks en LOD 0 sans couture.
    /// Appeler ensuite [`LodGrid::set_raw_lods`] puis [`LodGrid::rebalance`].
    pub fn new(coords: Vec<IVec2>) -> LodGrid {
        let Some(&first) = coords.first() else {
            return LodGrid {
                min: IVec2::ZERO,
                size: IVec2::ZERO,
                cells: Vec::new(),
                coords,
            };
        };
        let (mut min, mut max) = (first, first);
        for &c in &coords {
            min = min.min(c);
            max = max.max(c);
        }
        let size = max - min + IVec2::ONE;

        let mut grid = LodGrid {
            min,
            size,
            cells: vec![None; (size.x * size.y) as usize],
            coords,
        };
        for i in 0..grid.coords.len() {
            let c = grid.coords[i];
            let idx = grid.index(c).expect("coord dans sa propre boîte englobante");
            grid.cells[idx] = Some(Cell::default());
        }
        grid
    }

    /// Index plat de `c`, ou `None` s'il sort de la boîte englobante.
    #[inline]
    fn index(&self, c: IVec2) -> Option<usize> {
        let g = c - self.min;
        if g.x < 0 || g.y < 0 || g.x >= self.size.x || g.y >= self.size.y {
            return None;
        }
        Some((g.y * self.size.x + g.x) as usize)
    }

    #[inline]
    fn cell(&self, c: IVec2) -> Option<Cell> {
        self.index(c).and_then(|i| self.cells[i])
    }

    /// Le chunk `c` fait-il partie du monde chargé ? **Seul** critère de bord du monde
    /// pour le mailleur : la forme du monde (disque aujourd'hui, streaming demain) n'est
    /// décrite nulle part ailleurs que par l'ensemble des cases pleines.
    #[inline]
    pub fn is_loaded(&self, c: IVec2) -> bool {
        self.cell(c).is_some()
    }

    /// Niveau effectif (équilibré) du chunk `c`. **0 si absent** : un chunk hors monde
    /// n'est jamais « plus grossier », donc ne déclenche jamais de fausse couture au bord.
    #[inline]
    pub fn lod(&self, c: IVec2) -> u8 {
        self.cell(c).map_or(0, |x| x.lod)
    }

    /// Faces du chunk `c` portant une cellule de transition Transvoxel (vide si absent).
    #[inline]
    pub fn faces(&self, c: IVec2) -> TransitionFaces {
        self.cell(c).map_or(TransitionFaces::default(), |x| x.faces)
    }

    /// Recalcule le niveau **brut** de chaque chunk chargé : `f(coord, raw_courant)`.
    /// Passer le `raw` courant est ce qui permet à l'appelant d'appliquer une hystérésis.
    /// N'a aucun effet visible tant que [`LodGrid::rebalance`] n'a pas suivi.
    pub fn set_raw_lods(&mut self, mut f: impl FnMut(IVec2, u8) -> u8) {
        for i in 0..self.coords.len() {
            let c = self.coords[i];
            let Some(idx) = self.index(c) else { continue };
            if let Some(cell) = self.cells[idx].as_mut() {
                cell.raw = f(c, cell.raw);
            }
        }
    }

    /// Reconstruit `lod` et `faces` **depuis zéro** à partir des `raw` : équilibrage 2:1
    /// puis détection des coutures. À appeler après tout changement de `raw`.
    ///
    /// L'équilibrage n'est jamais une source de vérité — il repart des `raw` à chaque
    /// fois, sinon les niveaux dériveraient vers le bas passe après passe.
    pub fn rebalance(&mut self) {
        for cell in self.cells.iter_mut().flatten() {
            cell.lod = cell.raw;
        }
        self.balance();
        self.refresh_faces();
    }

    /// Relaxation 2:1 en place : tant qu'un chunk dépasse `voisin + MAX_LOD_STEP`, on le
    /// ramène à cette borne. Les cases hors monde ne contraignent rien.
    ///
    /// Converge : abaisser un chunk ne peut créer de violation que chez ses voisins, et
    /// les niveaux ne font que décroître, bornés par 0 ⇒ point fixe en au plus `MAX_LOD`
    /// passes.
    ///
    /// ⚠ **Aujourd'hui c'est un no-op**, et c'est voulu. Deux voisins ont leurs centres à
    /// 64 unités, donc leurs distances au focus diffèrent d'au plus 64 (inégalité
    /// triangulaire) : sauter un palier demanderait un anneau plus mince que 64, alors
    /// qu'ils font 220/400/1200 — même en tenant compte des ±8 % de l'hystérésis. On le
    /// garde parce qu'il sort en une passe quand rien ne viole (~40 µs sur 8000 chunks,
    /// une fois par franchissement de seuil) et que l'invariant tient par coïncidence de
    /// paramètres, pas par construction : resserrer un rayon, ajouter un palier, forcer un
    /// chunk fin parce qu'il est édité, ou passer au quadtree le casse en silence. En
    /// release le `debug_assert_eq!` de `mesh_chunk` ne rattrape rien : on obtient une
    /// fissure permanente, pas un crash.
    fn balance(&mut self) {
        loop {
            let mut changed = false;
            for i in 0..self.coords.len() {
                let c = self.coords[i];
                let Some(idx) = self.index(c) else { continue };
                let Some(mine) = self.cells[idx] else { continue };
                let limit = Face::ALL
                    .iter()
                    .filter_map(|f| self.cell(c + f.offset()))
                    .map(|n| n.lod + MAX_LOD_STEP)
                    .min()
                    .unwrap_or(mine.lod);
                if mine.lod > limit {
                    self.cells[idx] = Some(Cell {
                        lod: limit,
                        ..mine
                    });
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Masque de coutures de chaque chunk, dérivé des `lod` du voisinage. Doit venir
    /// **après** [`LodGrid::balance`] : le masque d'un chunk dépend de ses voisins.
    fn refresh_faces(&mut self) {
        for i in 0..self.coords.len() {
            let c = self.coords[i];
            let Some(idx) = self.index(c) else { continue };
            let my_lod = self.lod(c);
            let neighbors = Face::ALL.map(|f| self.lod(c + f.offset()));
            let faces = transition_faces(my_lod, neighbors);
            if let Some(cell) = self.cells[idx].as_mut() {
                cell.faces = faces;
            }
        }
    }

    /// Chunks à re-mailler pour passer de `prev` à `self` : ceux dont le couple
    /// **`(lod, faces)`** a changé, plus ceux qui viennent d'être chargés.
    ///
    /// Le masque compte autant que le niveau : quand A change de niveau, son voisin B
    /// garde le sien mais leur face commune devient (ou cesse d'être) une transition —
    /// or le rétrécissement demi-pas en dépend, donc **la géométrie de B change**.
    /// Diffé sur `lod` seul, on laisse une fissure permanente le long de la frontière.
    pub fn dirty_against(&self, prev: &LodGrid) -> Vec<IVec2> {
        self.coords
            .iter()
            .copied()
            .filter(|&c| match (self.cell(c), prev.cell(c)) {
                (Some(now), Some(old)) => (now.lod, now.faces) != (old.lod, old.faces),
                // Chunk nouvellement chargé (streaming) : rien à comparer, tout à mailler.
                _ => true,
            })
            .collect()
    }
}
