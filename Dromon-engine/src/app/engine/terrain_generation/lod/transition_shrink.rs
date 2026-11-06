// ─── Rétrécissement demi-pas ─────────────────────────────────────────────────
//
// Le long d'une face de transition, la dalle Transvoxel occupe la bande
// `[frontière − step/2, frontière]` : elle y redessine, à `step/2` en retrait, le contour
// que Marching Cubes trace sur le plan frontière lui-même. Deux nappes portant le même
// bord à deux profondeurs ⇒ elles se traversent.
//
// On récupère la place en COMPRIMANT la dernière rangée de sommets MC (et non en la
// supprimant : la rangée fait `step` d'épaisseur, la dalle `step/2` — la retirer
// changerait le recouvrement en fente). Avec `d` = distance au plan frontière :
//
//     d ↦ step/2 + d/2        (0 ≤ d ≤ step)
//
// La compression s'applique au rendu (`lib/chunk_seam.slang`). Ne reste ici que le choix
// des atténuations, qui lit les voisins dans la grille.
//
// ATTÉNUATION AUX EXTRÉMITÉS D'UNE FACE — indispensable, sinon des trous.
// Comprimer x près de la face est déplace aussi les sommets posés sur les faces nord et
// sud, qui sont des frontières partagées : si le voisin d'en face ne comprime pas pareil,
// les deux contours divergent ⇒ fente d'un carré `step/2` au coin. La règle qui évite ça :
// un chunk ne déforme sa surface QUE là où son voisin fait exactement la même chose.
// L'amplitude devient donc `h(t) = step/2 · τ(t)`, `t` = coordonnée tangente, `τ` montant
// de 0 à 1 sur `step` aux extrémités litigieuses :
//
//     d ↦ h + d·(step − h)/step        (0 ≤ d ≤ step)
//
// Les ruptures de pente de `τ` tombent sur la grille MC *et* sur les 9 points de la dalle
// ⇒ `h` est linéaire le long de toute arête interpolée : l'appliquer au sommet interpolé
// revient à l'appliquer à ses extrémités. Contrepartie : là où `τ` retombe à 0, la dalle
// est plate — d'où le pincement visible du ruban. On ne le paie qu'aux vraies marches
// d'escalier du champ de LOD, pas le long d'une frontière de transition rectiligne.

use glam::IVec2;

use crate::app::engine::terrain_generation::lod::{Face, TransitionFaces};

/// Bit d'atténuation de l'extrémité `end` (0 = basse, 1 = haute) de la face d'index `i`
/// dans [`Face::ALL`]. ⚠ Lu par `seamHasTaper` (`lib/chunk_seam.slang`).
#[inline]
fn taper_bit(i: usize, end: usize) -> u8 {
    1 << (2 * i + end)
}

/// Bits d'atténuation du chunk `coord` : pour chaque face portant une dalle, faut-il
/// atténuer à chaque extrémité ? Dépend des voisins **tangents**, donc indirectement des
/// diagonales.
///
/// `lod_of` et `faces_of` donnent le niveau et les faces d'un chunk quelconque : celles des
/// voisins entrent dans le calcul.
pub(super) fn taper_bits(
    coord: IVec2,
    lod_of: impl Fn(IVec2) -> u8,
    faces_of: impl Fn(IVec2) -> TransitionFaces,
) -> u8 {
    let lod = lod_of(coord);
    let faces = faces_of(coord);
    let mut bits = 0;
    for (i, f) in Face::ALL.into_iter().enumerate() {
        if !faces.contains(f) {
            continue;
        }
        // Voisins le long de l'axe TANGENT à la face : ce sont eux qui partagent le plan
        // sur lequel la compression déborde. Le test est symétrique (chacun pose la même
        // question à l'autre), donc les deux côtés concluent pareil.
        let tangent = if i < 2 { IVec2::Y } else { IVec2::X };
        for (end, s) in [-1, 1].into_iter().enumerate() {
            let nb = coord + tangent * s;
            if lod_of(nb) != lod || !faces_of(nb).contains(f) {
                bits |= taper_bit(i, end);
            }
        }
    }
    bits
}
