//! **Politique de LOD** du terrain — fonctions pures sur coordonnées/distances, sans
//! rien connaître du rendu ni du maillage. `world.rs` ne fait qu'orchestrer ; c'est ici
//! que vit la règle « distance → niveau de détail » (plus tard : anneaux, hystérésis).
//!
//! Un niveau de LOD `l` correspond à un pas d'échantillonnage `step = 1 << l` : LOD0 =
//! pleine résolution (÷1), LOD1 = ÷4 sommets, LOD2 = ÷16 (la surface est une nappe 2D,
//! doubler le pas quadruple l'aire couverte par cellule).

pub mod transition_cells;
pub mod transition_shrink;
mod transvoxel_tables;

use super::chunk::CHUNK_SIZE;
use glam::{IVec2, Vec2};
use std::collections::HashMap;

/// Niveau de détail maximal (pas = `1 << MAX_LOD`). 3 paliers pour commencer.
pub const MAX_LOD: u8 = 3;

/// Rayons (unités monde) des anneaux, mesurés depuis le point focal (caméra).
/// `dist < LOD1_DIST` → LOD0 (pleine résolution) ; `< LOD2_DIST` → LOD1 (÷4) ; au-delà → LOD2 (÷16).
const LOD1_DIST: f32 = 220.0;
const LOD2_DIST: f32 = 620.0;
const LOD3_DIST: f32 = 1820.0;

/// Distance horizontale (monde) du **centre** du chunk `coord` au point focal.
pub fn chunk_distance(coord: IVec2, focus: Vec2) -> f32 {
    let half = CHUNK_SIZE as f32 / 2.0;
    let cx = (coord.x * CHUNK_SIZE as i32) as f32 + half;
    let cy = (coord.y * CHUNK_SIZE as i32) as f32 + half;
    ((cx - focus.x).powi(2) + (cy - focus.y).powi(2)).sqrt()
}

/// LOD **statique** d'un chunk depuis un point focal : figé une fois à la génération
/// (étape 0). Le LOD dynamique (suivi caméra + hystérésis) viendra plus tard.
pub fn static_lod(coord: IVec2, focus: Vec2) -> u8 {
    let d = chunk_distance(coord, focus);
    if d < LOD1_DIST {
        0
    } else if d < LOD2_DIST {
        1
    } else if d < LOD3_DIST {
        2
    } else {
        3
    }
}

/// Écart de LOD maximal toléré entre deux chunks **voisins**. La cellule de transition
/// du Transvoxel suppose un voisin de pas exactement **double** (sa face grossière a
/// 2×2 coins face à 3×3 échantillons fins) : un écart de 2 niveaux n'est pas coudable.
const MAX_LOD_STEP: u8 = 1;

/// LOD de chaque chunk de `coords`, **équilibré 2:1** : c'est [`static_lod`] suivi d'une
/// relaxation qui affine tout chunk trop grossier pour un de ses voisins. Sans cette
/// contrainte (« octree restreint »), deux chunks adjacents peuvent différer de 2
/// niveaux — et le Transvoxel ne sait pas coudre ça.
///
/// La relaxation converge : abaisser le LOD d'un chunk ne peut créer de nouvelle
/// violation que chez ses voisins (jamais chez lui), et les niveaux ne font que
/// décroître, bornés par 0 ⇒ point fixe atteint en au plus `MAX_LOD` passes.
pub fn balanced_lods(coords: &[IVec2], focus: Vec2) -> Vec<u8> {
    let mut lods: HashMap<IVec2, u8> = coords.iter().map(|&c| (c, static_lod(c, focus))).collect();
    balance(coords, &mut lods);
    coords.iter().map(|c| lods[c]).collect()
}

/// Relaxation en place : tant qu'un chunk dépasse `voisin + MAX_LOD_STEP`, on le ramène
/// à cette borne. Les chunks absents de la carte (hors monde) ne contraignent rien.
fn balance(coords: &[IVec2], lods: &mut HashMap<IVec2, u8>) {
    loop {
        let mut changed = false;
        for &c in coords {
            let mine = lods[&c];
            let limit = Face::ALL
                .iter()
                .filter_map(|f| lods.get(&(c + f.offset())))
                .map(|&n| n + MAX_LOD_STEP)
                .min()
                .unwrap_or(mine);
            if mine > limit {
                lods.insert(c, limit);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

// ─── Détection des faces de transition (Transvoxel) ──────────────────────────
//
// Deux chunks voisins à des LOD différents laissent une fente sur leur face commune.
// On la scelle par une **cellule de transition** (cf. [`super::transvoxel_tables`])
// posée du côté HAUTE RÉSOLUTION : le chunk fin, celui qui a le plus de sommets à
// raccorder vers le bord grossier. Règle asymétrique — sur une face donnée, un seul
// des deux chunks (le plus fin) construit la transition : jamais les deux, jamais
// aucun. Chaque chunk est une colonne pleine hauteur tuilée en X/Y : ses seuls
// voisins sont horizontaux ⇒ 4 faces possibles (aucun voisin en Z).

/// Face horizontale d'un chunk, orientée vers son voisin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    NegX, // ouest
    PosX, // est
    NegY, // sud
    PosY, // nord
}

impl Face {
    /// Les 4 faces, dans l'ordre attendu par [`transition_faces`] pour `neighbor_lods`.
    pub const ALL: [Face; 4] = [Face::NegX, Face::PosX, Face::NegY, Face::PosY];

    /// Décalage (coordonnées chunk) vers le voisin situé de ce côté.
    pub fn offset(self) -> IVec2 {
        match self {
            Face::NegX => IVec2::new(-1, 0),
            Face::PosX => IVec2::new(1, 0),
            Face::NegY => IVec2::new(0, -1),
            Face::PosY => IVec2::new(0, 1),
        }
    }
}

/// Ensemble des faces d'un chunk portant une cellule de transition (bitmask, 1 bit par
/// [`Face`]). Le mailleur itérera dessus pour savoir quels bords coudre.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct TransitionFaces(u8);

impl TransitionFaces {
    /// La face `f` porte-t-elle une transition ?
    #[inline]
    pub fn contains(self, f: Face) -> bool {
        self.0 & (1 << f as u8) != 0
    }

    /// Aucune face à coudre (cas courant : intérieur d'un anneau de LOD).
    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Itère les faces actives (consommé par le mailleur aux étapes suivantes).
    pub fn iter(self) -> impl Iterator<Item = Face> {
        Face::ALL.into_iter().filter(move |&f| self.contains(f))
    }

    #[inline]
    fn set(&mut self, f: Face) {
        self.0 |= 1 << f as u8;
    }
}

/// **Règle de détection** (pure) : une face porte une transition ssi le voisin de ce
/// côté est PLUS GROSSIER (`neighbor_lod > my_lod`). `neighbor_lods` suit l'ordre de
/// [`Face::ALL`]. Un voisin plus fin ou de même LOD ⇒ rien (si transition il faut,
/// c'est l'autre chunk, le plus fin, qui la construit). Un voisin absent est passé en
/// LOD 0 par l'appelant : jamais plus grossier ⇒ pas de fausse transition au bord du
/// monde chargé.
pub fn transition_faces(my_lod: u8, neighbor_lods: [u8; 4]) -> TransitionFaces {
    let mut faces = TransitionFaces::default();
    for (&f, &nlod) in Face::ALL.iter().zip(neighbor_lods.iter()) {
        if nlod > my_lod {
            faces.set(f);
        }
    }
    faces
}
