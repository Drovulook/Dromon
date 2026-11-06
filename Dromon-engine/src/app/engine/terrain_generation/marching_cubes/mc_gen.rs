use glam::{IVec3, Vec3};
use rustc_hash::FxHashMap;

use crate::app::engine::{
    renderer::render_resources::TerrainVertex,
    terrain_generation::{
        chunk::ISO_LEVEL,
        generation::DensityField,
        marching_cubes::mc_tables::{CORNERS, EDGE_CORNERS},
        mesh::vertex::{EdgeKey, MaterialCache, NormalCache, cached_material, vertex_normal},
    },
};

/// Sommet mutualisé de l'arête de grille `e` du cube `base`. L'arête est identifiée
/// par ses deux coins entiers triés ([`EdgeKey`]) : la 1re fois on interpole la
/// position (là où la densité vaut ISO_LEVEL), on calcule normale+matériaux et on pousse
/// le sommet ; les fois suivantes (cube voisin, autre triangle) on réutilise l'index.
/// Renvoie l'index du sommet.
#[allow(clippy::too_many_arguments)]
pub fn edge_vertex(
    e: usize,
    base: [i32; 3],
    step: i32,
    corner_d: &[f32; 8],
    field: &DensityField,
    vertices: &mut Vec<TerrainVertex>,
    vertex_map: &mut FxHashMap<EdgeKey, u32>,
    normal_cache: &mut NormalCache,
    surface_materials: &mut MaterialCache,
) -> u32 {
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
        return idx;
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
    // Le sommet reste sur l'iso-surface : la compression de la couture se fait au rendu
    // (`lib/chunk_seam.slang`).
    let p = pa + (pb - pa) * t;

    let (a, b) = (IVec3::from_array(ia), IVec3::from_array(ib));
    let normal = vertex_normal(field, normal_cache, a, b, t, step);
    let material = cached_material(surface_materials, p, step, |q| {
        field.surface_material(q, normal, field.edge_macro_gradient(a, b, t, step))
    });
    let idx = vertices.len() as u32;
    vertices.push(TerrainVertex::new(p, normal, material));
    vertex_map.insert(key, idx);
    idx
}
