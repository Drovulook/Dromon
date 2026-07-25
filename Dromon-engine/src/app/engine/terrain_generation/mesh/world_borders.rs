use glam::{IVec2, Vec3};
use rustc_hash::FxHashMap;

const GENERATE_WORLD_WALLS: bool = false;
const GENERATE_WORLD_BOTTOM: bool = true;

use crate::app::engine::{
    renderer::render_resources::TerrainVertex,
    terrain_generation::{
        chunk::CHUNK_SIZE,
        generation::DensityField,
        mesh::{mesher::WORLD_FLOOR, vertex::cached_color},
    },
};

/// Étendue du monde en **coordonnées chunk** (bornes incluses). Sert au mailleur à
/// savoir quels côtés d'un chunk sont au bord du monde — donc à fermer par des parois.
#[derive(Clone, Copy)]
pub struct WorldBounds {
    pub min: IVec2,
    pub max: IVec2,
}

/// Ajoute la face du dessous (à [`WORLD_FLOOR`]) et, pour chaque côté du chunk situé
/// au bord du monde, une paroi verticale montant du fond jusqu'au relief.
pub fn add_mesh_borders(
    field: &DensityField,
    coord: IVec2,
    bounds: WorldBounds,
    step: i32,
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    volume_colors: &mut FxHashMap<(i32, i32, i32), Vec3>,
) {
    let n = CHUNK_SIZE as i32;
    let x0 = coord.x * n;
    let y0 = coord.y * n;
    let x1 = x0 + n;
    let y1 = y0 + n;
    let f = WORLD_FLOOR as f32;

    if GENERATE_WORLD_BOTTOM {
        // Fond : un quad plat couvrant le chunk, normale vers le bas. Visible du dessous
        // partout, d'où sa présence sur tous les chunks (pas seulement au bord).
        push_quad(
            vertices,
            indices,
            field,
            volume_colors,
            [
                Vec3::new(x0 as f32, y0 as f32, f),
                Vec3::new(x1 as f32, y0 as f32, f),
                Vec3::new(x1 as f32, y1 as f32, f),
                Vec3::new(x0 as f32, y1 as f32, f),
            ],
            Vec3::new(0.0, 0.0, -1.0),
        );
    }

    if GENERATE_WORLD_WALLS {
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
                    volume_colors,
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
}

/// Émet un quad `[a, b, c, d]` (sommets dans l'ordre du contour) en deux triangles,
/// avec une normale de face constante. Couleurs échantillonnées au champ (strates
/// visibles en coupe sur les parois).
fn push_quad(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    field: &DensityField,
    volume_colors: &mut FxHashMap<(i32, i32, i32), Vec3>,
    quad: [Vec3; 4],
    normal: Vec3,
) {
    let [a, b, c, d] = quad;
    push_tri(vertices, indices, field, volume_colors, a, b, c, normal);
    push_tri(vertices, indices, field, volume_colors, a, c, d, normal);
}

/// Émet un triangle avec normale de face imposée, en orientant l'enroulement pour que
/// la normale géométrique aille dans le sens de `normal` (même convention que la
/// surface MC : la face regarde vers l'extérieur/le vide).
fn push_tri(
    vertices: &mut Vec<TerrainVertex>,
    indices: &mut Vec<u32>,
    field: &DensityField,
    volume_colors: &mut FxHashMap<(i32, i32, i32), Vec3>,
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
        let color = cached_color(volume_colors, p, |q| field.volume_color(q));
        vertices.push(TerrainVertex {
            pos: p,
            normal,
            color,
        });
    }
    indices.extend_from_slice(&[start, start + 1, start + 2]);
}
