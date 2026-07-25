// ─── Cellules de transition (Transvoxel) ─────────────────────────────────────
//
// Sur la face commune à un chunk fin et un chunk grossier, le fin échantillonne le
// champ deux fois plus serré : ses bords ne coïncident pas avec ceux du voisin → fente.
// La cellule de transition est une **dalle mince** posée par le chunk FIN le long de
// cette face, avec deux bords différents :
//   - côté voisin (`side = 1`) : seulement 4 coins, aux positions exactes qu'échantillonne
//     le grossier ⇒ bord identique au sien, étanche PAR CONSTRUCTION ;
//   - côté intérieur (`side = 0`) : les 9 échantillons fins ⇒ se raccorde à la surface MC.
// Des triangles dans l'épaisseur cousent les deux bords.
//
// Deux points qui surprennent :
//  1. Les 4 coins grossiers ne sont PAS de nouveaux échantillons : ils occupent les
//     mêmes positions monde que les points 0/2/6/8 de la grille fine, donc portent les
//     mêmes densités. D'où un code de cas sur 9 bits (et pas 13).
//  2. Le choix du repère `(u, v)` de la face est LIBRE. Un repère miroir de celui de
//     Lengyel fait lire le cas miroir dans la table, dont la triangulation — replacée
//     dans notre repère miroir — redonne la bonne géométrie, à l'enroulement près (que
//     l'on recalcule de toute façon par le gradient). Seule compte la COHÉRENCE : mêmes
//     `u`/`v` pour le code de cas et pour la position des 13 points.

use glam::{IVec2, IVec3, Vec3};
use rustc_hash::FxHashMap;

use crate::app::engine::{
    renderer::render_resources::TerrainVertex,
    terrain_generation::{
        chunk::{CHUNK_SIZE, ISO_LEVEL},
        generation::DensityField,
        lod::{
            Face,
            transition_shrink::HalfStepShrink,
            transvoxel_tables::{
                TRANSITION_CASE_BITS, TRANSITION_CELL_CLASS, TRANSITION_CELL_DATA,
                TRANSITION_CORNER_UV, transition_edge,
            },
        },
        mesh::mesher::{DEBUG_TRANSITION_COLOR, EdgeKey, cached_color, vertex_normal},
    },
};

/// Repère local d'une face de transition. `origin` = coin `(u=0, v=0)` de la face sur le
/// **plan frontière** — le plan que le voisin grossier échantillonne aussi ; `u`/`v` =
/// axes du plan ; `inward` = normale entrant dans le chunk fin.
struct FaceFrame {
    origin: IVec3,
    u: IVec3,
    v: IVec3,
    inward: IVec3,
}

/// Repère de la face `face` du chunk `coord`. Les 4 faces sont verticales ⇒ `v = +Z`
/// pour toutes, et `origin.z = 0` (la boucle ajoute directement le z monde le long de `v`).
fn face_frame(coord: IVec2, face: Face) -> FaceFrame {
    let n = CHUNK_SIZE as i32;
    let x0 = coord.x * n;
    let y0 = coord.y * n;
    let (origin, u, inward) = match face {
        Face::NegX => (IVec3::new(x0, y0, 0), IVec3::Y, IVec3::X),
        Face::PosX => (IVec3::new(x0 + n, y0, 0), IVec3::Y, IVec3::NEG_X),
        Face::NegY => (IVec3::new(x0, y0, 0), IVec3::X, IVec3::Y),
        Face::PosY => (IVec3::new(x0, y0 + n, 0), IVec3::X, IVec3::NEG_Y),
    };
    FaceFrame {
        origin,
        u,
        v: IVec3::Z,
        inward,
    }
}

/// Position d'un des 13 points, en **demi-unités monde** (position doublée). Le doublage
/// n'est pas cosmétique : les 9 points de la face haute réso sont rentrés d'un DEMI-pas
/// fin dans le chunk (sinon la dalle, à cheval sur le seul plan frontière, serait
/// d'épaisseur nulle → triangles dégénérés) ; en demi-unités, ce demi-pas s'écrit en
/// entier, donc la position sert telle quelle de clé de mutualisation exacte.
#[inline]
fn transition_point(k: usize, plane: &[IVec3; 13], step: i32, inward: IVec3) -> IVec3 {
    let inset = if TRANSITION_CORNER_UV[k][2] == 0 {
        inward * step // = 2 × (step / 2), en demi-unités.
    } else {
        IVec3::ZERO // face basse réso : reste sur le plan frontière.
    };
    plane[k] * 2 + inset
}

/// Coud la face `face` du chunk : une cellule de transition par carré de côté `2·step`
/// (le pas du voisin), sur toute la hauteur utile.
#[allow(clippy::too_many_arguments)]
pub fn add_transition_cells(
    field: &DensityField,
    coord: IVec2,
    face: Face,
    step: i32,
    shrink: HalfStepShrink,
    bounds_z: (i32, i32),
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    trans_map: &mut FxHashMap<EdgeKey, u32>,
    normal_cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
    surface_colors: &mut FxHashMap<(i32, i32, i32), Vec3>,
) {
    let n = CHUNK_SIZE as i32;
    let frame = face_frame(coord, face);

    // Côté d'une cellule de transition = le pas du voisin GROSSIER (2 × le nôtre). C'est
    // ce qui fait tomber ses 4 coins pile sur les échantillons du voisin.
    let cell = 2 * step;

    // Même piège de calage qu'en MC, mais sur le treillis du VOISIN (multiples de `cell`) :
    // c'est lui qui impose la grille de la face commune.
    let (z_min, z_max) = bounds_z;
    let z_start = z_min.div_euclid(cell) * cell;

    let mut plane = [IVec3::ZERO; 13];
    let mut dens = [0.0f32; 13];

    for a in (0..n).step_by(cell as usize) {
        for b in (z_start..=z_max).step_by(cell as usize) {
            // Position (sur le plan frontière) et densité des 13 points. Les 4 coins
            // grossiers retombent sur les positions de 0/2/6/8 → mêmes valeurs, sans
            // cas particulier : c'est exactement ce qui rend le bord identique au voisin.
            for k in 0..13 {
                let [cu, cv, _] = TRANSITION_CORNER_UV[k];
                let p = frame.origin
                    + frame.u * (a + cu as i32 * step)
                    + frame.v * (b + cv as i32 * step);
                plane[k] = p;
                dens[k] = field.sample(p.x, p.y, p.z);
            }

            // Code de cas 9 bits — convention INVERSE de Bourke : bit posé = coin PLEIN.
            let mut case = 0usize;
            for (bit, &k) in TRANSITION_CASE_BITS.iter().enumerate() {
                if dens[k] > ISO_LEVEL {
                    case |= 1 << bit;
                }
            }

            let class = (TRANSITION_CELL_CLASS[case] & 0x7F) as usize;
            let data = &TRANSITION_CELL_DATA[class];
            let vertex_count = data.vertex_count() as usize;
            if vertex_count == 0 {
                continue; // face entièrement pleine ou entièrement vide.
            }

            // Référence d'enroulement : `−∇d` du champ à l'échelle de la CELLULE, lu sur
            // les 9 points fins (0..8, grille 3×3 centrée en 4). Surtout pas les normales
            // de sommet : leur stencil reste à 5 unités quel que soit le LOD, donc à pas
            // grossier elles décrivent un relief plus fin que la géométrie et peuvent
            // s'écarter de plus de 90° du triangle → triangles retournés (trous noirs).
            // Les 9 points sont coplanaires ⇒ `winding_ref` n'a aucune composante selon la
            // normale de la face ; sans importance, celle-ci est verticale et la normale du
            // terrain y est très majoritairement contenue.
            let centre = plane[4].as_vec3();
            let mut winding_ref = Vec3::ZERO;
            for k in 0..9 {
                winding_ref -= (plane[k].as_vec3() - centre) * dens[k];
            }

            // Sommets de la cellule (≤ 12), puis triangles par triplets d'indices LOCAUX
            // à cette liste. On retient les positions pour orienter comme en MC.
            let mut vi = [0u32; 12];
            let mut vp = [Vec3::ZERO; 12];
            for k in 0..vertex_count {
                let (c0, c1) = transition_edge(case, k);
                let (idx, p, _) = transition_vertex(
                    c0 as usize,
                    c1 as usize,
                    &plane,
                    &dens,
                    step,
                    frame.inward,
                    face,
                    shrink,
                    field,
                    vertices,
                    trans_map,
                    normal_cache,
                    surface_colors,
                );
                vi[k] = idx;
                vp[k] = p;
            }

            for t in 0..data.triangle_count() as usize {
                let (i0, i1, i2) = (
                    data.vertex_index[3 * t] as usize,
                    data.vertex_index[3 * t + 1] as usize,
                    data.vertex_index[3 * t + 2] as usize,
                );
                let geo = (vp[i1] - vp[i0]).cross(vp[i2] - vp[i0]);
                if geo.dot(winding_ref) < 0.0 {
                    indices.extend_from_slice(&[vi[i0], vi[i2], vi[i1]]);
                } else {
                    indices.extend_from_slice(&[vi[i0], vi[i1], vi[i2]]);
                }
            }
        }
    }
}

/// Sommet mutualisé posé sur l'arête `c0–c1` d'une cellule de transition (indices parmi
/// les 13 points). Même schéma qu'[`edge_vertex`] — interpolation linéaire à l'iso, puis
/// normale/couleur mémoïsées — mais en **demi-unités monde** : les positions y sont
/// entières malgré le rentrement d'un demi-pas, donc utilisables telles quelles comme
/// clé exacte (cf. [`transition_point`]).
#[allow(clippy::too_many_arguments)]
fn transition_vertex(
    c0: usize,
    c1: usize,
    plane: &[IVec3; 13],
    dens: &[f32; 13],
    step: i32,
    inward: IVec3,
    face: Face,
    shrink: HalfStepShrink,
    field: &DensityField,
    vertices: &mut Vec<TerrainVertex>,
    trans_map: &mut FxHashMap<EdgeKey, u32>,
    normal_cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
    surface_colors: &mut FxHashMap<(i32, i32, i32), Vec3>,
) -> (u32, Vec3, Vec3) {
    // Aucune arête de la table ne relie la face haute réso à la basse (vérifié sur les 512
    // cas) : un sommet appartient donc entièrement à l'une ou à l'autre. C'est ce qui rend
    // le test de `side` sur le seul `c0` légitime plus bas.
    debug_assert_eq!(TRANSITION_CORNER_UV[c0][2], TRANSITION_CORNER_UV[c1][2]);
    let ha = transition_point(c0, plane, step, inward);
    let hb = transition_point(c1, plane, step, inward);
    // Clé canonique : les deux extrémités triées (indépendante de l'ordre dans la table).
    let key = if ha.to_array() <= hb.to_array() {
        (ha.x, ha.y, ha.z, hb.x, hb.y, hb.z)
    } else {
        (hb.x, hb.y, hb.z, ha.x, ha.y, ha.z)
    };
    if let Some(&idx) = trans_map.get(&key) {
        let v = vertices[idx as usize];
        return (idx, v.pos, v.normal);
    }

    // Positions réelles des deux extrémités : le point du plan, rentré dans le chunk de
    // l'épaisseur LOCALE de la dalle (atténuée près des bords, nulle sur la face basse
    // réso — qui doit rester sur le plan pour épouser le voisin). Les demi-unités `ha/hb`
    // ne servaient qu'à la clé de mutualisation.
    let inward_f = inward.as_vec3();
    let corner = |k: usize| -> Vec3 {
        let p = plane[k].as_vec3();
        if TRANSITION_CORNER_UV[k][2] == 0 {
            p + inward_f * shrink.inset_at(face, p)
        } else {
            p
        }
    };
    let pa = corner(c0);
    let pb = corner(c1);
    let (da, db) = (dens[c0], dens[c1]);
    let denom = db - da;
    let t = if denom.abs() < 1e-6 {
        0.5
    } else {
        (ISO_LEVEL - da) / denom
    };
    // Point d'échantillonnage : l'interpolé sur le PLAN FRONTIÈRE, sans le retrait — c'est
    // lui qui est sur l'iso-surface (même raison qu'en MC, cf. [`edge_vertex`]), et c'est
    // exactement le point que calcule le sommet MC jumeau ⇒ normale et couleur identiques
    // des deux côtés de la soudure.
    let pf = |k: usize| plane[k].as_vec3();
    let p_field = pf(c0) + (pf(c1) - pf(c0)) * t;

    // Position émise : simplement l'interpolé sur les points déjà rentrés. Aucune correction
    // tangentielle — avec l'atténuation, la surface MC ne bouge plus DANS le plan d'une face
    // de transition (l'amplitude de l'axe tangent y est nulle), donc la dalle n'a rien à
    // suivre latéralement.
    let p = pa + (pb - pa) * t;

    let normal = vertex_normal(field, normal_cache, p_field);
    let color = if DEBUG_TRANSITION_COLOR {
        Vec3::new(1.0, 0.0, 1.0)
    } else {
        cached_color(surface_colors, p_field, |q| field.surface_color(q))
    };
    let idx = vertices.len() as u32;
    vertices.push(TerrainVertex {
        pos: p,
        normal,
        color,
    });
    trans_map.insert(key, idx);
    (idx, p, normal)
}
