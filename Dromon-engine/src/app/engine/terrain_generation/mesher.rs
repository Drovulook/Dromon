use super::chunk::{CHUNK_SIZE, ISO_LEVEL};
use super::chunk_manager::ChunkManager;
use super::density_field::DensityField;
use super::marching_cubes::{CORNERS, EDGE_CORNERS, TRI_TABLE};
use crate::app::engine::renderer::render_resources::TerrainVertex;
use crate::profile;
use glam::{IVec2, Vec3};
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

    // Champ de densité échantillonnable sur la région du chunk. L'apron vaut le rayon
    // des normales : le stencil de différences ne débordera jamais du pré-échantillon.
    let field = manager.density_field(coord, NORMAL_RADIUS);

    // Tranche verticale à mailler, fournie par le champ : hors d'elle, tout est plein
    // (dessous) ou vide (dessus). Se resserre automatiquement autour de la surface —
    // et s'élargira vers le bas quand les grottes creuseront en profondeur.
    let (z_min, z_max) = field.vertical_bounds();

    let mut vertices: Vec<TerrainVertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // Normales et couleurs sont coûteuses (gradient multi-rayon, choix de matériau).
    // On les mémoïse par coin entier : les nombreux sommets voisins d'un chunk
    // retombent sur les mêmes clés → calcul fait ~une fois par colonne. `FxHashMap`
    // (hash non-cryptographique) au lieu du `HashMap` SipHash : bien plus rapide sur
    // ces clés entières.
    let mut normal_cache: FxHashMap<(i32, i32, i32), Vec3> = FxHashMap::default();
    let mut color_cache: FxHashMap<(i32, i32, i32), Vec3> = FxHashMap::default();

    let mut corner_d = [0.0f32; 8];

    for lx in 0..n {
        for ly in 0..n {
            for lz in z_min..=z_max {
                let base = [origin_x + lx, origin_y + ly, lz];

                // Densité aux 8 coins → index de cas (bit posé quand le coin est
                // sous l'iso = vide, convention de la table de Bourke).
                let mut cube_index = 0usize;
                for (i, off) in CORNERS.iter().enumerate() {
                    let d = field.sample(base[0] + off[0], base[1] + off[1], base[2] + off[2]);
                    corner_d[i] = d;
                    if d < ISO_LEVEL {
                        cube_index |= 1 << i;
                    }
                }

                let tris = &TRI_TABLE[cube_index];
                if tris[0] < 0 {
                    continue; // cube entièrement plein ou entièrement vide.
                }

                // Position d'un sommet sur l'arête `e`, là où la densité vaut
                // ISO_LEVEL (interpolation linéaire entre les deux coins).
                let interp = |e: usize| -> Vec3 {
                    let [a, b] = EDGE_CORNERS[e];
                    let (da, db) = (corner_d[a], corner_d[b]);
                    let ca = CORNERS[a];
                    let cb = CORNERS[b];
                    let pa = Vec3::new(
                        (base[0] + ca[0]) as f32,
                        (base[1] + ca[1]) as f32,
                        (base[2] + ca[2]) as f32,
                    );
                    let pb = Vec3::new(
                        (base[0] + cb[0]) as f32,
                        (base[1] + cb[1]) as f32,
                        (base[2] + cb[2]) as f32,
                    );
                    let denom = db - da;
                    let t = if denom.abs() < 1e-6 {
                        0.5
                    } else {
                        (ISO_LEVEL - da) / denom
                    };
                    pa + (pb - pa) * t
                };

                let mut t = 0;
                while t + 2 < 16 && tris[t] >= 0 {
                    let p0 = interp(tris[t] as usize);
                    let p1 = interp(tris[t + 1] as usize);
                    let p2 = interp(tris[t + 2] as usize);

                    let n0 = vertex_normal(&field, &mut normal_cache, p0);
                    let n1 = vertex_normal(&field, &mut normal_cache, p1);
                    let n2 = vertex_normal(&field, &mut normal_cache, p2);

                    // On oriente le triangle d'après le gradient de densité plutôt
                    // que de se fier au sens d'enroulement de la table : la normale
                    // géométrique (produit vectoriel) doit pointer vers le vide, comme
                    // les normales de sommet. Sinon on inverse les deux derniers
                    // sommets. Robuste quelle que soit la convention de la table.
                    let geo = (p1 - p0).cross(p2 - p0);
                    let (p1, p2, n1, n2) = if geo.dot(n0 + n1 + n2) < 0.0 {
                        (p2, p1, n2, n1)
                    } else {
                        (p1, p2, n1, n2)
                    };

                    let c0 = vertex_color(&field, &mut color_cache, p0);
                    let c1 = vertex_color(&field, &mut color_cache, p1);
                    let c2 = vertex_color(&field, &mut color_cache, p2);

                    let start = vertices.len() as u32;
                    vertices.push(TerrainVertex {
                        pos: p0,
                        normal: n0,
                        color: c0,
                    });
                    vertices.push(TerrainVertex {
                        pos: p1,
                        normal: n1,
                        color: c1,
                    });
                    vertices.push(TerrainVertex {
                        pos: p2,
                        normal: n2,
                        color: c2,
                    });
                    indices.extend_from_slice(&[start, start + 1, start + 2]);

                    t += 3;
                }
            }
        }
    }

    // Fermeture du volume : fond (partout) + parois sur les côtés qui touchent le bord
    // du monde. Ces faces sont planes et explicites (pas de Marching Cubes) → murs nets,
    // et elles se cousent à la surface car leur arête haute est à `z = relief` aux mêmes
    // points entiers que les sommets de bord du maillage MC.
    add_mesh_borders(
        &field,
        coord,
        bounds,
        &mut vertices,
        &mut indices,
        &mut color_cache,
    );

    (vertices, indices)
}

/// Ajoute la face du dessous (à [`WORLD_FLOOR`]) et, pour chaque côté du chunk situé
/// au bord du monde, une paroi verticale montant du fond jusqu'au relief.
fn add_mesh_borders(
    field: &DensityField,
    coord: IVec2,
    bounds: WorldBounds,
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    color_cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
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
        color_cache,
        [
            Vec3::new(x0 as f32, y0 as f32, f),
            Vec3::new(x1 as f32, y0 as f32, f),
            Vec3::new(x1 as f32, y1 as f32, f),
            Vec3::new(x0 as f32, y1 as f32, f),
        ],
        Vec3::new(0.0, 0.0, -1.0),
    );

    // Paroi verticale le long d'une arête, subdivisée par colonne (un quad du fond au
    // relief entre deux colonnes voisines). `along_x` = l'arête court selon x (donc x
    // varie, y fixé) ; sinon elle court selon y.
    let mut wall = |along_x: bool, fixed: i32, normal: Vec3| {
        for k in 0..n {
            let (a0, a1) = if along_x {
                ((x0 + k, fixed), (x0 + k + 1, fixed))
            } else {
                ((fixed, y0 + k), (fixed, y0 + k + 1))
            };
            let top0 = field.surface_z(a0.0, a0.1);
            let top1 = field.surface_z(a1.0, a1.1);
            push_quad(
                vertices,
                indices,
                field,
                color_cache,
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
    color_cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
    quad: [Vec3; 4],
    normal: Vec3,
) {
    let [a, b, c, d] = quad;
    push_tri(vertices, indices, field, color_cache, a, b, c, normal);
    push_tri(vertices, indices, field, color_cache, a, c, d, normal);
}

/// Émet un triangle avec normale de face imposée, en orientant l'enroulement pour que
/// la normale géométrique aille dans le sens de `normal` (même convention que la
/// surface MC : la face regarde vers l'extérieur/le vide).
fn push_tri(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    field: &DensityField,
    color_cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
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
        let color = vertex_color(field, color_cache, p);
        vertices.push(TerrainVertex {
            pos: p,
            normal,
            color,
        });
    }
    indices.extend_from_slice(&[start, start + 1, start + 2]);
}

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

/// Couleur du sommet `p` : matériau de surface résolu côté CPU (mélange des
/// dominants). Échantillonné au coin entier le plus proche et mémoïsé. Fréquence
/// bien plus basse que la densité (un appel par sommet unique).
fn vertex_color(
    field: &DensityField,
    cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
    p: Vec3,
) -> Vec3 {
    let key = (p.x.round() as i32, p.y.round() as i32, p.z.round() as i32);
    if let Some(&c) = cache.get(&key) {
        return c;
    }
    let c = field.color(key.0, key.1, key.2);
    cache.insert(key, c);
    c
}
