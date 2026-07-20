//! **Politique de LOD** du terrain — fonctions pures sur coordonnées/distances, sans
//! rien connaître du rendu ni du maillage. `world.rs` ne fait qu'orchestrer ; c'est ici
//! que vit la règle « distance → niveau de détail » (plus tard : anneaux, hystérésis).
//!
//! Un niveau de LOD `l` correspond à un pas d'échantillonnage `step = 1 << l` : LOD0 =
//! pleine résolution (÷1), LOD1 = ÷4 sommets, LOD2 = ÷16 (la surface est une nappe 2D,
//! doubler le pas quadruple l'aire couverte par cellule).

use super::chunk::CHUNK_SIZE;
use glam::{IVec2, Vec2};

/// Niveau de détail maximal (pas = `1 << MAX_LOD`). 3 paliers pour commencer.
pub const MAX_LOD: u8 = 3;

/// Rayons (unités monde) des anneaux, mesurés depuis le point focal (caméra).
/// `dist < LOD1_DIST` → LOD0 (pleine résolution) ; `< LOD2_DIST` → LOD1 (÷4) ; au-delà → LOD2 (÷16).
const LOD1_DIST: f32 = 220.0;
const LOD2_DIST: f32 = 520.0;
const LOD3_DIST: f32 = 2020.0;

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
