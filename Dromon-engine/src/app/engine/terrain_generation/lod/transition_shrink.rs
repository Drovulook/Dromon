// ─── Rétrécissement demi-pas ─────────────────────────────────────────────────
//
// Le long d'une face de transition, la dalle Transvoxel occupe déjà la bande
// `[frontière − step/2, frontière]` : elle y redessine, à `step/2` en retrait, le
// contour que Marching Cubes trace sur le plan frontière lui-même. Deux nappes
// portant le même bord à deux profondeurs ⇒ elles se traversent.
//
// On récupère la place en COMPRIMANT la dernière rangée de sommets MC (et non en la
// supprimant : la rangée fait `step` d'épaisseur, la dalle `step/2` — la retirer
// changerait le recouvrement en fente). Avec `d` = distance au plan frontière :
//
//     d ↦ step/2 + d/2        (0 ≤ d ≤ step)
//
// `d = 0` → `step/2` : le bord extérieur vient épouser la face haute réso de la dalle.
// `d = step` → `step` : le bord intérieur ne bouge pas, continuité avec le chunk.
//
// La soudure est EXACTE, pas approchée : le sommet MC du plan frontière et le sommet
// fin de la dalle sortent de la même arête, avec les mêmes densités (la dalle
// échantillonne le champ sur le plan frontière et ne décale que les positions) et la
// même interpolation linéaire. Une fois comprimés, ils sont confondus.
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
// Les ruptures de pente de `τ` tombent en `t0+step` et `t0+n−step`, points de la grille
// MC *et* des 9 points de la dalle ⇒ `h` est linéaire le long de toute arête interpolée,
// donc la position interpolée d'un sommet de dalle coïncide encore avec `h` évalué en ce
// sommet. Contrepartie : là où `τ` retombe à 0, la dalle est plate — d'où le pincement
// visible du ruban. On ne le paie qu'aux vraies marches d'escalier du champ de LOD (cf.
// `PlaneShrink::taper`), pas le long d'une frontière de transition rectiligne.

use glam::{IVec2, Vec3};

use crate::app::engine::terrain_generation::{
    ChunkManager,
    chunk::CHUNK_SIZE,
    lod::{Face, TransitionFaces},
};

/// Une face portant une dalle, du point de vue du rétrécissement.
#[derive(Clone, Copy)]
struct PlaneShrink {
    /// Coordonnée monde du plan frontière.
    plane: f32,
    /// Atténuer à l'extrémité tangentielle basse / haute ? Inutile quand le voisin d'en
    /// face comprime à l'identique — même LOD *et* dalle sur la même face.
    taper: [bool; 2],
}

/// Compression de la dernière rangée de sommets MC bordant une face de transition.
/// Sans effet sur un chunk qui n'en porte aucune (cas courant).
#[derive(Clone, Copy, Default)]
pub struct HalfStepShrink {
    step: f32,
    /// Une entrée par face, dans l'ordre de [`Face::ALL`] — donc `[NegX, PosX, NegY, PosY]`.
    faces: [Option<PlaneShrink>; 4],
    /// Coin `(x0, y0)` du chunk : origine des rampes d'atténuation.
    origin: [f32; 2],
}

impl HalfStepShrink {
    pub fn new(manager: &ChunkManager, coord: IVec2, faces: TransitionFaces, step: i32) -> Self {
        let n = CHUNK_SIZE as i32;
        let lod = manager.chunk_lod(coord);
        let mut slots = [None; 4];
        for (i, (slot, f)) in slots.iter_mut().zip(Face::ALL).enumerate() {
            if !faces.contains(f) {
                continue;
            }
            let plane = match f {
                Face::NegX => coord.x * n,
                Face::PosX => (coord.x + 1) * n,
                Face::NegY => coord.y * n,
                Face::PosY => (coord.y + 1) * n,
            };
            // Voisins le long de l'axe TANGENT à la face : ce sont eux qui partagent le
            // plan sur lequel la compression déborde. Le test est symétrique (chacun pose
            // la même question à l'autre), donc les deux côtés concluent pareil.
            let tangent = if i < 2 { IVec2::Y } else { IVec2::X };
            let taper = [-1, 1].map(|s| {
                let nb = coord + tangent * s;
                manager.chunk_lod(nb) != lod || !manager.chunk_transition_faces(nb).contains(f)
            });
            *slot = Some(PlaneShrink {
                plane: plane as f32,
                taper,
            });
        }
        Self {
            step: step as f32,
            faces: slots,
            origin: [(coord.x * n) as f32, (coord.y * n) as f32],
        }
    }

    /// Position corrigée d'un sommet MC. Z n'est jamais touché : les chunks sont des
    /// colonnes pleine hauteur, aucun voisin (donc aucune dalle) au-dessus ni en dessous.
    /// Chaque axe lit son amplitude sur la coordonnée d'ORIGINE de l'autre (déplacement
    /// simultané, pas séquentiel).
    #[inline]
    pub fn apply(&self, p: Vec3) -> Vec3 {
        Vec3::new(self.axis(p.x, 0, p.y), self.axis(p.y, 2, p.x), p.z)
    }

    /// Épaisseur de la dalle de `face` à la position `p` : exactement l'amplitude du
    /// retrait subi par la surface MC au même endroit, d'où la soudure.
    #[inline]
    pub fn inset_at(&self, face: Face, p: Vec3) -> f32 {
        let i = Self::index(face);
        self.amount(i, if i < 2 { p.y } else { p.x })
    }

    #[inline]
    fn index(face: Face) -> usize {
        match face {
            Face::NegX => 0,
            Face::PosX => 1,
            Face::NegY => 2,
            Face::PosY => 3,
        }
    }

    /// Amplitude du retrait pour la face `i`, à la coordonnée tangente `t`.
    #[inline]
    fn amount(&self, i: usize, t: f32) -> f32 {
        let Some(f) = self.faces[i] else { return 0.0 };
        // Faces X (i < 2) → axe tangent Y, et réciproquement.
        let t0 = if i < 2 {
            self.origin[1]
        } else {
            self.origin[0]
        };
        let mut r = 1.0f32;
        if f.taper[0] {
            r = r.min((t - t0) / self.step);
        }
        if f.taper[1] {
            r = r.min((t0 + CHUNK_SIZE as f32 - t) / self.step);
        }
        0.5 * self.step * r.clamp(0.0, 1.0)
    }

    /// Compression le long d'un axe. `neg` = index de sa face négative dans [`Face::ALL`]
    /// (0 pour X, 2 pour Y) ; la positive suit immédiatement. `t` = coordonnée tangente,
    /// qui pilote l'atténuation. Les deux bandes ne peuvent pas se chevaucher
    /// (`2·step ≤ 16 < CHUNK_SIZE`), d'où le `return` dès qu'une s'applique.
    #[inline]
    fn axis(&self, c: f32, neg: usize, t: f32) -> f32 {
        for (i, sign) in [(neg, 1.0f32), (neg + 1, -1.0f32)] {
            let Some(f) = self.faces[i] else { continue };
            let d = (c - f.plane) * sign;
            if d < self.step {
                let h = self.amount(i, t);
                if h <= 0.0 {
                    return c;
                }
                return f.plane + sign * (h + d * (self.step - h) / self.step);
            }
        }
        c
    }
}
