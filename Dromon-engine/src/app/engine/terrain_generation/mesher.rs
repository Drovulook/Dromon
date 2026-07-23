use super::chunk::{CHUNK_SIZE, ISO_LEVEL};
use super::chunk_manager::ChunkManager;
use super::density_field::DensityField;
use super::lod::{Face, TransitionFaces};
use super::marching_cubes::{CORNERS, EDGE_CORNERS, TRI_TABLE};
use super::transvoxel_tables::{
    TRANSITION_CASE_BITS, TRANSITION_CELL_CLASS, TRANSITION_CELL_DATA, TRANSITION_CORNER_UV,
    transition_edge,
};
use crate::app::engine::renderer::render_resources::TerrainVertex;
use crate::profile;
use glam::{IVec2, IVec3, Vec3};
use rustc_hash::FxHashMap;

// ─── Mailleur volumétrique (Marching Cubes) ──────────────────────────────────
//
// On **extrait l'iso-surface** d'un champ de densité 3D ([`DensityField`]). Le
// mailleur ne connaît QUE ce champ : `sample(x,y,z)` (la densité) et
// `vertical_bounds()` (la tranche en z à parcourir). Il ignore tout du relief, des
// grottes, des édits — c'est le champ qui les porte. Ajouter des grottes ne touchera
// donc pas ce fichier : dès que `DensityField::sample` renverra du vide en
// profondeur, Marching Cubes y posera des triangles automatiquement (grottes,
// surplombs, cavités détachées), dans le même buffer de chunk.
//
// Principe : pour chaque cube de la grille, on regarde le signe de la densité à ses
// 8 coins ; là où la surface traverse une arête, on pose un sommet par interpolation
// linéaire. Couture sans fissure : la densité ne dépend que de la position MONDE,
// donc deux chunks voisins calculent des coins identiques sur leur face partagée.

/// Rayon (en voxels) du stencil de différences pour les normales, moyenné sur
/// `k = 1..=NORMAL_RADIUS`. Plus grand = normale lissée sur une zone plus large
/// (éclairage doux, moins d'aliasing sur les pentes). Dimensionne aussi l'« apron »
/// que le champ de densité doit pré-échantillonner autour du chunk.
const NORMAL_RADIUS: i32 = 5;

/// Altitude du fond du monde (plancher plein). En dessous, on pose la face du
/// dessous ; les parois de bordure montent de là jusqu'au relief.
const WORLD_FLOOR: i32 = 0;

/// DEBUG : ne coudre que cette face (valider une direction à la fois). `None` = les 4.
const DEBUG_TRANSITION_FACE: Option<Face> = None;

/// DEBUG : teinte en magenta les sommets des cellules de transition, pour voir où
/// elles se posent (et vérifier qu'elles longent bien les frontières de LOD).
const DEBUG_TRANSITION_COLOR: bool = true;

/// Clé d'une arête de la grille : ses deux coins entiers, **triés** (pour que deux
/// cubes voisins partageant l'arête tombent sur la même clé, quel que soit l'ordre de
/// leurs coins). Sert à mutualiser les sommets Marching Cubes : un seul sommet par
/// arête, réutilisé par tous ses triangles (surface indexée ⇒ ~×5 de sommets en moins).
type EdgeKey = (i32, i32, i32, i32, i32, i32);

/// Étendue du monde en **coordonnées chunk** (bornes incluses). Sert au mailleur à
/// savoir quels côtés d'un chunk sont au bord du monde — donc à fermer par des parois.
#[derive(Clone, Copy)]
pub struct WorldBounds {
    pub min: IVec2,
    pub max: IVec2,
}

/// Construit le mesh d'un chunk en coordonnées monde (`model` = identité au draw) :
/// la surface (Marching Cubes) plus, aux bords du monde, les parois latérales et le
/// fond (cf. [`WorldBounds`]) qui ferment le volume.
pub fn mesh_chunk(
    manager: &ChunkManager,
    coord: IVec2,
    bounds: WorldBounds,
) -> (Vec<TerrainVertex>, Vec<u32>) {
    profile!();
    let n = CHUNK_SIZE as i32;
    let origin_x = coord.x * n;
    let origin_y = coord.y * n;

    // Pas d'échantillonnage MC = `1 << lod` (1, 2, 4…). Il divise CHUNK_SIZE, donc les
    // chunks se tuilent proprement. N'affecte QUE la géométrie : le stencil des normales
    // reste à un pas fixe de 1 unité (éclairage continu à travers une frontière de LOD).
    let step = 1i32 << manager.chunk_lod(coord);

    // Champ de densité échantillonnable sur la région du chunk. L'apron vaut le rayon
    // des normales : le stencil de différences ne débordera jamais du pré-échantillon.
    let field = manager.density_field(coord, NORMAL_RADIUS);

    // Tranche verticale à mailler, fournie par le champ : hors d'elle, tout est plein
    // (dessous) ou vide (dessus). Se resserre automatiquement autour de la surface —
    // et s'élargira vers le bas quand les grottes creuseront en profondeur.
    let (z_min, z_max) = field.vertical_bounds();

    // Calage du départ z sur un treillis global multiple de `step`. En X/Y les colonnes
    // tombent déjà sur un treillis global (origin = coord·64, 64 divisible par step) ;
    // pas en Z. Sans ce cadrage, deux chunks voisins dont les `z_min` ont des parités
    // différentes échantillonneraient des z décalés sur leur face commune → fissure même
    // à LOD égal. `div_euclid` arrondit vers le bas au multiple de step (sûr si négatif).
    let z_start = z_min.div_euclid(step) * step;

    let mut vertices: Vec<TerrainVertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // Normales et couleurs sont coûteuses (gradient multi-rayon, choix de matériau).
    // On les mémoïse par coin entier : les nombreux sommets voisins d'un chunk
    // retombent sur les mêmes clés → calcul fait ~une fois par colonne. `FxHashMap`
    // (hash non-cryptographique) au lieu du `HashMap` SipHash : bien plus rapide sur
    // ces clés entières.
    //
    // Deux caches de couleur, car deux règles distinctes : l'iso-surface est sur le toit
    // (profondeur nulle), les parois le coupent (strates à leur profondeur réelle). Un
    // cache commun les mélangerait — le haut d'une paroi est à `z ≈ relief`, donc à la
    // même clé arrondie qu'un sommet de surface.
    let mut normal_cache: FxHashMap<(i32, i32, i32), Vec3> = FxHashMap::default();
    let mut surface_colors: FxHashMap<(i32, i32, i32), Vec3> = FxHashMap::default();
    let mut strata_colors: FxHashMap<(i32, i32, i32), Vec3> = FxHashMap::default();

    // Mutualisation des sommets : arête de grille → index. Chaque arête ne produit
    // qu'un sommet, partagé par tous les cubes/triangles qui la touchent.
    let mut vertex_map: FxHashMap<EdgeKey, u32> = FxHashMap::default();

    // Faces à coudre — connues AVANT le maillage : elles conditionnent le
    // rétrécissement demi-pas que subit la dernière rangée de sommets qui les borde.
    let faces = manager.chunk_transition_faces(coord);
    let shrink = HalfStepShrink::new(coord, faces, step);

    let mut corner_d = [0.0f32; 8];

    for lx in (0..n).step_by(step as usize) {
        for ly in (0..n).step_by(step as usize) {
            for lz in (z_start..=z_max).step_by(step as usize) {
                let base = [origin_x + lx, origin_y + ly, lz];

                // Densité aux 8 coins → index de cas (bit posé quand le coin est
                // sous l'iso = vide, convention de la table de Bourke). Les coins sont
                // espacés de `step` (cube MC de côté `step` unités monde).
                let mut cube_index = 0usize;
                for (i, off) in CORNERS.iter().enumerate() {
                    let d = field.sample(
                        base[0] + off[0] * step,
                        base[1] + off[1] * step,
                        base[2] + off[2] * step,
                    );
                    corner_d[i] = d;
                    if d < ISO_LEVEL {
                        cube_index |= 1 << i;
                    }
                }

                let tris = &TRI_TABLE[cube_index];
                if tris[0] < 0 {
                    continue; // cube entièrement plein ou entièrement vide.
                }

                // `edge(e)` = sommet mutualisé de l'arête `e` du cube courant. La
                // clôture referme les 6 arguments d'état (buffers + caches) communs aux
                // 3 appels. `edge_vertex` renvoie des valeurs possédées → aucun emprunt
                // de `vertices` ne fuit hors de l'appel (sinon le test d'orientation, qui
                // lit `vertices`, entrerait en conflit avec l'emprunt `&mut`).
                let mut edge = |e: usize| {
                    edge_vertex(
                        e,
                        base,
                        step,
                        shrink,
                        &corner_d,
                        &field,
                        &mut vertices,
                        &mut vertex_map,
                        &mut normal_cache,
                        &mut surface_colors,
                    )
                };

                let mut t = 0;
                while t + 2 < 16 && tris[t] >= 0 {
                    let (i0, p0, n0) = edge(tris[t] as usize);
                    let (i1, p1, n1) = edge(tris[t + 1] as usize);
                    let (i2, p2, n2) = edge(tris[t + 2] as usize);

                    // Oriente l'enroulement d'après le gradient (normale géométrique vers
                    // le vide, comme les normales de sommet) : on permute les INDEX.
                    let geo = (p1 - p0).cross(p2 - p0);
                    if geo.dot(n0 + n1 + n2) < 0.0 {
                        indices.extend_from_slice(&[i0, i2, i1]);
                    } else {
                        indices.extend_from_slice(&[i0, i1, i2]);
                    }

                    t += 3;
                }
            }
        }
    }

    // Fermeture du volume : fond (partout) + parois sur les côtés qui touchent le bord
    // du monde. Ces faces sont planes et explicites (pas de Marching Cubes) → murs nets,
    // et elles se cousent à la surface car leur arête haute est à `z = relief` aux mêmes
    // points entiers que les sommets de bord du maillage MC.
    //
    // Elles ignorent `shrink` : le bord du monde est le point le plus éloigné du focus,
    // donc uniformément au LOD le plus grossier ⇒ aucun voisin plus grossier, aucune
    // dalle sur ces faces, rien à rétrécir. À revoir si le focus peut s'en approcher.
    add_mesh_borders(
        &field,
        coord,
        bounds,
        step,
        &mut vertices,
        &mut indices,
        &mut strata_colors,
    );

    // Cellules de transition (Transvoxel) : scellent les faces bordant un voisin PLUS
    // GROSSIER. Table de mutualisation dédiée — leurs positions sont en demi-unités,
    // clés incompatibles avec celles du MC. Aucun index n'est donc partagé avec la
    // surface ci-dessus, mais le rétrécissement demi-pas a rendu les positions
    // confondues (cf. [`HalfStepShrink`]) : la couture ferme quand même.
    if !faces.is_empty() {
        let mut trans_map: FxHashMap<EdgeKey, u32> = FxHashMap::default();
        for face in faces.iter() {
            if DEBUG_TRANSITION_FACE.is_some_and(|only| only != face) {
                continue;
            }
            debug_assert_eq!(
                manager.chunk_lod(coord + face.offset()),
                manager.chunk_lod(coord) + 1,
                "Transvoxel ne coud qu'un niveau d'écart (cf. lod::balanced_lods)"
            );
            add_transition_cells(
                &field,
                coord,
                face,
                step,
                shrink,
                (z_min, z_max),
                &mut vertices,
                &mut indices,
                &mut trans_map,
                &mut normal_cache,
                &mut surface_colors,
            );
        }
    }

    (vertices, indices)
}

/// Ajoute la face du dessous (à [`WORLD_FLOOR`]) et, pour chaque côté du chunk situé
/// au bord du monde, une paroi verticale montant du fond jusqu'au relief.
fn add_mesh_borders(
    field: &DensityField,
    coord: IVec2,
    bounds: WorldBounds,
    step: i32,
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    strata_colors: &mut FxHashMap<(i32, i32, i32), Vec3>,
) {
    let n = CHUNK_SIZE as i32;
    let x0 = coord.x * n;
    let y0 = coord.y * n;
    let x1 = x0 + n;
    let y1 = y0 + n;
    let f = WORLD_FLOOR as f32;

    // Fond : un quad plat couvrant le chunk, normale vers le bas. Visible du dessous
    // partout, d'où sa présence sur tous les chunks (pas seulement au bord).
    push_quad(
        vertices,
        indices,
        field,
        strata_colors,
        [
            Vec3::new(x0 as f32, y0 as f32, f),
            Vec3::new(x1 as f32, y0 as f32, f),
            Vec3::new(x1 as f32, y1 as f32, f),
            Vec3::new(x0 as f32, y1 as f32, f),
        ],
        Vec3::new(0.0, 0.0, -1.0),
    );

    // Paroi verticale le long d'une arête, subdivisée par pas `step` (un quad du fond au
    // relief entre deux colonnes distantes de `step`). Au pas `step`, le haut du mur suit
    // `surface_z` aux mêmes colonnes que les sommets MC de bord → coïncidence exacte
    // (densité linéaire en z ⇒ le sommet MC tombe pile à `z = relief`), pas de fente.
    // `along_x` = l'arête court selon x (donc x varie, y fixé) ; sinon elle court selon y.
    let mut wall = |along_x: bool, fixed: i32, normal: Vec3| {
        for k in (0..n).step_by(step as usize) {
            let (a0, a1) = if along_x {
                ((x0 + k, fixed), (x0 + k + step, fixed))
            } else {
                ((fixed, y0 + k), (fixed, y0 + k + step))
            };
            let top0 = field.surface_z(a0.0, a0.1);
            let top1 = field.surface_z(a1.0, a1.1);
            push_quad(
                vertices,
                indices,
                field,
                strata_colors,
                [
                    Vec3::new(a0.0 as f32, a0.1 as f32, f),
                    Vec3::new(a1.0 as f32, a1.1 as f32, f),
                    Vec3::new(a1.0 as f32, a1.1 as f32, top1),
                    Vec3::new(a0.0 as f32, a0.1 as f32, top0),
                ],
                normal,
            );
        }
    };

    if coord.x == bounds.min.x {
        wall(false, x0, Vec3::new(-1.0, 0.0, 0.0)); // ouest
    }
    if coord.x == bounds.max.x {
        wall(false, x1, Vec3::new(1.0, 0.0, 0.0)); // est
    }
    if coord.y == bounds.min.y {
        wall(true, y0, Vec3::new(0.0, -1.0, 0.0)); // sud
    }
    if coord.y == bounds.max.y {
        wall(true, y1, Vec3::new(0.0, 1.0, 0.0)); // nord
    }
}

/// Émet un quad `[a, b, c, d]` (sommets dans l'ordre du contour) en deux triangles,
/// avec une normale de face constante. Couleurs échantillonnées au champ (strates
/// visibles en coupe sur les parois).
fn push_quad(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    field: &DensityField,
    strata_colors: &mut FxHashMap<(i32, i32, i32), Vec3>,
    quad: [Vec3; 4],
    normal: Vec3,
) {
    let [a, b, c, d] = quad;
    push_tri(vertices, indices, field, strata_colors, a, b, c, normal);
    push_tri(vertices, indices, field, strata_colors, a, c, d, normal);
}

/// Émet un triangle avec normale de face imposée, en orientant l'enroulement pour que
/// la normale géométrique aille dans le sens de `normal` (même convention que la
/// surface MC : la face regarde vers l'extérieur/le vide).
fn push_tri(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    field: &DensityField,
    strata_colors: &mut FxHashMap<(i32, i32, i32), Vec3>,
    p0: Vec3,
    p1: Vec3,
    p2: Vec3,
    normal: Vec3,
) {
    let geo = (p1 - p0).cross(p2 - p0);
    let (p1, p2) = if geo.dot(normal) < 0.0 {
        (p2, p1)
    } else {
        (p1, p2)
    };
    let start = vertices.len() as u32;
    for p in [p0, p1, p2] {
        let color = cached_color(strata_colors, p, |q| field.color(q));
        vertices.push(TerrainVertex {
            pos: p,
            normal,
            color,
        });
    }
    indices.extend_from_slice(&[start, start + 1, start + 2]);
}

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

/// Compression de la dernière rangée de sommets MC bordant une face de transition.
/// Sans effet sur un chunk qui n'en porte aucune (cas courant).
#[derive(Clone, Copy, Default)]
struct HalfStepShrink {
    step: f32,
    /// Plan frontière (coordonnée monde) de chaque face portant une dalle, dans
    /// l'ordre de [`Face::ALL`] — donc `[NegX, PosX, NegY, PosY]`.
    planes: [Option<f32>; 4],
}

impl HalfStepShrink {
    fn new(coord: IVec2, faces: TransitionFaces, step: i32) -> Self {
        let n = CHUNK_SIZE as i32;
        let mut planes = [None; 4];
        for (slot, f) in planes.iter_mut().zip(Face::ALL) {
            if !faces.contains(f) || DEBUG_TRANSITION_FACE.is_some_and(|only| only != f) {
                continue;
            }
            let plane = match f {
                Face::NegX => coord.x * n,
                Face::PosX => (coord.x + 1) * n,
                Face::NegY => coord.y * n,
                Face::PosY => (coord.y + 1) * n,
            };
            *slot = Some(plane as f32);
        }
        Self {
            step: step as f32,
            planes,
        }
    }

    /// Position corrigée d'un sommet MC. Z n'est jamais touché : les chunks sont des
    /// colonnes pleine hauteur, aucun voisin (donc aucune dalle) au-dessus ni en dessous.
    #[inline]
    fn apply(&self, p: Vec3) -> Vec3 {
        Vec3::new(self.axis(p.x, 0), self.axis(p.y, 2), p.z)
    }

    /// Compression restreinte aux axes **tangents** à `face`. Pour les sommets de la face
    /// haute réso d'une dalle : leur retrait dans la direction normale est déjà posé par
    /// [`transition_point`], mais leurs coordonnées DANS le plan doivent suivre le même
    /// écrasement que la surface MC à laquelle ils se soudent. Sans ça, la couture s'ouvre
    /// là où deux faces de transition se rejoignent — le coin, où la rangée MC est
    /// comprimée sur les deux axes alors que chaque dalle n'en connaît qu'un.
    #[inline]
    fn apply_tangential(&self, p: Vec3, face: Face) -> Vec3 {
        match face {
            Face::NegX | Face::PosX => Vec3::new(p.x, self.axis(p.y, 2), p.z),
            Face::NegY | Face::PosY => Vec3::new(self.axis(p.x, 0), p.y, p.z),
        }
    }

    /// Compression le long d'un axe. `neg` = index de sa face négative dans
    /// [`Face::ALL`] (0 pour X, 2 pour Y) ; la positive suit immédiatement. Les deux
    /// bandes ne peuvent pas se chevaucher (`2·step ≤ 16 < CHUNK_SIZE`), d'où le
    /// `return` dès qu'une s'applique.
    #[inline]
    fn axis(&self, c: f32, neg: usize) -> f32 {
        if let Some(plane) = self.planes[neg] {
            let d = c - plane;
            if d < self.step {
                return plane + (self.step + d) * 0.5;
            }
        }
        if let Some(plane) = self.planes[neg + 1] {
            let d = plane - c;
            if d < self.step {
                return plane - (self.step + d) * 0.5;
            }
        }
        c
    }
}

/// Sommet mutualisé de l'arête de grille `e` du cube `base`. L'arête est identifiée
/// par ses deux coins entiers triés ([`EdgeKey`]) : la 1re fois on interpole la
/// position (là où la densité vaut ISO_LEVEL), on calcule normale+couleur et on pousse
/// le sommet ; les fois suivantes (cube voisin, autre triangle) on réutilise l'index.
/// Renvoie `(index, position, normale)` — la position/normale servent au test
/// d'orientation de l'appelant sans re-lire le buffer.
#[allow(clippy::too_many_arguments)]
fn edge_vertex(
    e: usize,
    base: [i32; 3],
    step: i32,
    shrink: HalfStepShrink,
    corner_d: &[f32; 8],
    field: &DensityField,
    vertices: &mut Vec<TerrainVertex>,
    vertex_map: &mut FxHashMap<EdgeKey, u32>,
    normal_cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
    surface_colors: &mut FxHashMap<(i32, i32, i32), Vec3>,
) -> (u32, Vec3, Vec3) {
    let [a, b] = EDGE_CORNERS[e];
    let ca = CORNERS[a];
    let cb = CORNERS[b];
    // Coins espacés de `step` (mêmes positions monde que l'échantillonnage des densités).
    // La clé reste les deux coins entiers triés → dédoublonnage correct dans le chunk.
    let ia = [
        base[0] + ca[0] * step,
        base[1] + ca[1] * step,
        base[2] + ca[2] * step,
    ];
    let ib = [
        base[0] + cb[0] * step,
        base[1] + cb[1] * step,
        base[2] + cb[2] * step,
    ];
    // Clé canonique : les deux coins triés (indépendante de l'ordre a/b propre au cube).
    let key = if ia <= ib {
        (ia[0], ia[1], ia[2], ib[0], ib[1], ib[2])
    } else {
        (ib[0], ib[1], ib[2], ia[0], ia[1], ia[2])
    };
    if let Some(&idx) = vertex_map.get(&key) {
        let v = vertices[idx as usize];
        return (idx, v.pos, v.normal);
    }

    // Interpolation linéaire sur l'arête (identique à l'ancien `interp`).
    let (da, db) = (corner_d[a], corner_d[b]);
    let pa = Vec3::new(ia[0] as f32, ia[1] as f32, ia[2] as f32);
    let pb = Vec3::new(ib[0] as f32, ib[1] as f32, ib[2] as f32);
    let denom = db - da;
    let t = if denom.abs() < 1e-6 {
        0.5
    } else {
        (ISO_LEVEL - da) / denom
    };
    // Deux positions distinctes, et c'est volontaire : `p_field` est le point interpolé,
    // donc SUR l'iso-surface ; `p` est ce point comprimé, donc décollé d'elle. Normale et
    // couleur s'échantillonnent sur `p_field`, exactement comme le fait la dalle ⇒ les deux
    // côtés de la couture évaluent le même point, aucune rupture d'éclairage ni de matériau.
    // La clé de mutualisation, elle, reste les coins entiers d'origine : le sommet partagé
    // avec le cube voisin garde le même index.
    let p_field = pa + (pb - pa) * t;
    let p = shrink.apply(p_field);

    let normal = vertex_normal(field, normal_cache, p_field);
    let color = cached_color(surface_colors, p_field, |q| field.surface_color(q));
    let idx = vertices.len() as u32;
    vertices.push(TerrainVertex {
        pos: p,
        normal,
        color,
    });
    vertex_map.insert(key, idx);
    (idx, p, normal)
}

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
fn add_transition_cells(
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

            // Sommets de la cellule (≤ 12), puis triangles par triplets d'indices LOCAUX
            // à cette liste. On retient position/normale pour orienter comme en MC.
            let mut vi = [0u32; 12];
            let mut vp = [Vec3::ZERO; 12];
            let mut vn = [Vec3::ZERO; 12];
            for k in 0..vertex_count {
                let (c0, c1) = transition_edge(case, k);
                let (idx, p, nrm) = transition_vertex(
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
                vn[k] = nrm;
            }

            for t in 0..data.triangle_count() as usize {
                let (i0, i1, i2) = (
                    data.vertex_index[3 * t] as usize,
                    data.vertex_index[3 * t + 1] as usize,
                    data.vertex_index[3 * t + 2] as usize,
                );
                let geo = (vp[i1] - vp[i0]).cross(vp[i2] - vp[i0]);
                if geo.dot(vn[i0] + vn[i1] + vn[i2]) < 0.0 {
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

    // Retour en unités monde (les demi-unités ne servent qu'à la clé).
    let pa = ha.as_vec3() * 0.5;
    let pb = hb.as_vec3() * 0.5;
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

    // Position émise : l'interpolé sur les points rentrés, puis comprimé dans le PLAN de la
    // face (jamais selon sa normale — le retrait y est déjà fait). Seule la face haute réso
    // est concernée ; la basse doit rester intacte pour épouser le voisin.
    let p = pa + (pb - pa) * t;
    let p = if TRANSITION_CORNER_UV[c0][2] == 0 {
        shrink.apply_tangential(p, face)
    } else {
        p
    };

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

//
//
//
//

/// Normale de surface au sommet `p` : `−∇density` normalisé (pointe vers le vide, où
/// la densité décroît). Gradient estimé par différences centrées **moyennées** sur
/// `k = 1..=NORMAL_RADIUS` (mélange de pas pairs et impairs → pas de damier
/// d'aliasing). Échantillonné au coin entier le plus proche et mémoïsé.
fn vertex_normal(
    field: &DensityField,
    cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
    p: Vec3,
) -> Vec3 {
    let key = (p.x.round() as i32, p.y.round() as i32, p.z.round() as i32);
    if let Some(&n) = cache.get(&key) {
        return n;
    }
    let (wx, wy, wz) = key;
    let r = NORMAL_RADIUS.max(1);
    let (mut gx, mut gy, mut gz) = (0.0f32, 0.0f32, 0.0f32);
    for k in 1..=r {
        let s = (2 * k) as f32;
        gx += (field.sample(wx + k, wy, wz) - field.sample(wx - k, wy, wz)) / s;
        gy += (field.sample(wx, wy + k, wz) - field.sample(wx, wy - k, wz)) / s;
        gz += (field.sample(wx, wy, wz + k) - field.sample(wx, wy, wz - k)) / s;
    }
    // Le facteur de moyenne 1/r disparaît à la normalisation ; on l'omet.
    let mut n = Vec3::new(-gx, -gy, -gz).normalize_or_zero();
    if n == Vec3::ZERO {
        n = Vec3::Z; // sécurité si le gradient est nul (surface plate dégénérée).
    }
    cache.insert(key, n);
    n
}

/// Couleur du sommet `p`, mémoïsée au coin entier le plus proche : les sommets voisins
/// y partagent une couleur quasi identique, économie sans artefact visible. Le calcul
/// lui-même prend la position **flottante** exacte.
///
/// `eval` porte la règle de coloration, donc le choix du cache qui va avec :
/// [`DensityField::surface_color`] pour l'iso-surface, [`DensityField::color`] pour les
/// parois et le fond (cf. `surface_colors` / `strata_colors` dans [`mesh_chunk`]).
fn cached_color(
    cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
    p: Vec3,
    eval: impl FnOnce(Vec3) -> Vec3,
) -> Vec3 {
    let key = (p.x.round() as i32, p.y.round() as i32, p.z.round() as i32);
    if let Some(&c) = cache.get(&key) {
        return c;
    }
    let c = eval(p);
    cache.insert(key, c);
    c
}

#[cfg(test)]
mod tests {
    use super::super::chunk_manager::GenParams;
    use super::super::lod::transition_faces;
    use super::*;

    /// Maille deux chunks voisins de LOD 0 et 1 (le fin à l'ouest) et rend leurs sommets.
    /// Le fin porte donc une face de transition vers l'est, le grossier aucune.
    ///
    /// Les bornes du monde débordent volontairement la paire : aucun des deux n'est au
    /// bord, donc aucune paroi latérale ne vient parasiter le plan frontière que les
    /// tests inspectent (seule y reste la surface, plus le fond à `WORLD_FLOOR`).
    fn stitched_pair() -> (Vec<TerrainVertex>, Vec<TerrainVertex>) {
        let (fine, coarse) = (IVec2::new(0, 0), IVec2::new(1, 0));
        let mut manager = ChunkManager::new(GenParams::default());
        manager.generate_chunk(fine);
        manager.generate_chunk(coarse);
        manager.set_chunk_lod(coarse, 1);
        manager.refresh_transition_faces(fine);
        manager.refresh_transition_faces(coarse);

        let bounds = WorldBounds {
            min: IVec2::new(-1, -1),
            max: IVec2::new(2, 2),
        };
        (
            mesh_chunk(&manager, fine, bounds).0,
            mesh_chunk(&manager, coarse, bounds).0,
        )
    }

    /// La face de transition est bien détectée sur le plan partagé (garde-fou : sans
    /// elle, le test d'étanchéité ci-dessous passerait pour de mauvaises raisons).
    #[test]
    fn fine_chunk_stitches_toward_the_coarse_neighbor() {
        let mut manager = ChunkManager::new(GenParams::default());
        manager.generate_chunk(IVec2::new(0, 0));
        manager.generate_chunk(IVec2::new(1, 0));
        manager.set_chunk_lod(IVec2::new(1, 0), 1);
        manager.refresh_transition_faces(IVec2::new(0, 0));

        let faces = manager.chunk_transition_faces(IVec2::new(0, 0));
        assert!(faces.contains(Face::PosX));
        assert_eq!(faces.iter().count(), 1);
    }

    /// **Étanchéité de la couture.** Tout sommet que le chunk GROSSIER pose sur le plan
    /// frontière doit se retrouver chez le fin : la face basse réso de la dalle reprend
    /// les 4 coins du voisin, donc ses arêtes ET ses interpolations — le bord est le même
    /// *par construction*, ce qui est toute la raison d'être du Transvoxel.
    ///
    /// Comparaison à tolérance (et non bit-à-bit) : les deux côtés interpolent la même
    /// arête, mais rien ne garantit qu'ils la parcourent dans le même sens, et
    /// `a + (b - a)·t` n'est pas bit-identique à `b + (a - b)·(1 - t)` en flottant.
    #[test]
    fn transition_reproduces_the_coarse_border() {
        let (fine, coarse) = stitched_pair();
        let border = CHUNK_SIZE as f32;

        let on_border: Vec<Vec3> = fine
            .iter()
            .map(|v| v.pos)
            .filter(|p| (p.x - border).abs() < 1e-3)
            .collect();

        let mut checked = 0;
        for v in coarse.iter().filter(|v| (v.pos.x - border).abs() < 1e-3) {
            assert!(
                on_border.iter().any(|p| p.distance(v.pos) < 1e-3),
                "sommet {:?} du bord grossier absent de la couture → fente",
                v.pos
            );
            checked += 1;
        }
        assert!(
            checked > 10,
            "seulement {checked} sommets sur la frontière : le test ne prouve rien"
        );
    }

    /// La formule de compression, isolée : le plan frontière recule d'un demi-pas (là
    /// où la dalle pose sa face haute réso), le fond de la rangée ne bouge pas
    /// (continuité avec le reste du chunk), et au-delà d'un pas rien n'est touché.
    #[test]
    fn half_step_shrink_compresses_only_the_border_row() {
        let step = 4;
        let border = CHUNK_SIZE as f32; // face est du chunk (0,0)
        let shrink = HalfStepShrink::new(IVec2::ZERO, transition_faces(0, [0, 1, 0, 0]), step);
        // Sommet à distance `d` du plan frontière → sa coordonnée x après compression.
        let x = |d: f32| shrink.apply(Vec3::new(border - d, 10.0, 10.0)).x;

        assert_eq!(x(0.0), border - 2.0); // bord extérieur : recule de step/2
        assert_eq!(x(2.0), border - 3.0); // milieu : rangée comprimée ×2, linéairement
        assert_eq!(x(step as f32), border - 4.0); // bord intérieur : inchangé
        assert_eq!(x(9.0), border - 9.0); // hors rangée : intact

        // Rien sur les axes sans dalle (ici NegX, NegY, PosY) ni jamais sur Z.
        let p = Vec3::new(10.0, 0.0, 0.0);
        assert_eq!(shrink.apply(p), p);
    }

    /// **Soudure intérieure** — le pendant de [`transition_reproduces_the_coarse_border`],
    /// qui ne teste que la face BASSE réso de la dalle (côté voisin). Ici la face HAUTE
    /// réso : le rétrécissement doit avoir vidé le plan frontière de ses sommets de
    /// surface, tous reculés à `frontière − step/2` — précisément là où la dalle pose les
    /// siens, sur les mêmes arêtes et avec les mêmes densités. Chacun y a donc un jumeau.
    #[test]
    fn shrunk_border_welds_onto_the_transition_slab() {
        let (fine, _) = stitched_pair();
        let border = CHUNK_SIZE as f32;
        let step = 1.0; // le chunk fin est en LOD 0.

        // Le fond du monde touche le plan frontière et n'est pas rétréci (aucune dalle
        // en face de lui) : on ne garde que la surface.
        let surface: Vec<Vec3> = fine
            .iter()
            .map(|v| v.pos)
            .filter(|p| p.z > WORLD_FLOOR as f32 + 1.0)
            .collect();

        // Le plan frontière lui-même n'est PAS vide : la face basse réso de la dalle y
        // reste, c'est elle qui épouse le voisin. Ce qui doit avoir disparu, c'est la
        // bande ouverte `(frontière − step/2, frontière)` : la compression envoie tout
        // sommet MC à une distance ≥ `step/2` du plan, et la dalle n'y pose rien.
        assert!(
            !surface
                .iter()
                .any(|p| p.x > border - step / 2.0 + 1e-4 && p.x < border - 1e-4),
            "un sommet de surface occupe la bande que la dalle traverse → recouvrement"
        );

        let welded: Vec<Vec3> = surface
            .into_iter()
            .filter(|p| (p.x - (border - step / 2.0)).abs() < 1e-4)
            .collect();
        assert!(
            welded.len() > 10,
            "seulement {} sommets sur le plan rétréci : le test ne prouve rien",
            welded.len()
        );

        for p in &welded {
            // ≥ 2 : le sommet MC et celui de la dalle, confondus mais d'index distincts
            // (tables de mutualisation séparées). `p` se compte lui-même.
            let twins = welded.iter().filter(|q| p.distance(**q) < 1e-4).count();
            assert!(
                twins >= 2,
                "sommet {p} seul sur le plan rétréci → couture ouverte côté intérieur"
            );
        }
    }
}
