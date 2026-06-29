use super::chunk::CHUNK_SIZE;
use super::chunk_manager::ChunkManager;
use crate::app::engine::renderer::render_resources::TerrainVertex;
use glam::{IVec2, Vec3};

/// Construit le mesh de **surface** d'un chunk, en coordonnées monde.
///
/// Approche heightmap : une grille de `(CHUNK_SIZE + 1)²` sommets, un par
/// colonne, placé à la hauteur de surface. On déborde d'**une** colonne sur le
/// chunk voisin (d'où le `+ 1`) pour que les bords coïncident exactement :
/// le sommet de droite du chunk `(0, 0)` est le même point monde que le sommet
/// de gauche du chunk `(1, 0)` → pas de fissure entre chunks.
///
/// Le mesh étant déjà en coordonnées monde, la matrice `model` poussée au draw
/// est l'identité. Si un voisin n'est pas encore généré (bord extérieur du
/// monde), la hauteur retombe sur celle de la colonne de bord du chunk courant
/// (petit liseré plat, plutôt qu'une falaise ou un trou).
pub fn mesh_chunk(manager: &ChunkManager, coord: IVec2) -> (Vec<TerrainVertex>, Vec<u32>) {
    let size = CHUNK_SIZE as i32;
    let origin_x = coord.x * size;
    let origin_y = coord.y * size;
    let dim = CHUNK_SIZE + 1; // nombre de sommets par côté

    let mut vertices = Vec::with_capacity(dim * dim);
    for gy in 0..dim {
        for gx in 0..dim {
            let wx = origin_x + gx as i32;
            let wy = origin_y + gy as i32;

            // Hauteur réelle à (wx, wy) ; à défaut (voisin absent), on clampe
            // l'échantillon à l'intérieur du chunk courant.
            let (z, weights) = manager.surface_at(wx, wy).unwrap_or_else(|| {
                let cx = origin_x + (gx.min(CHUNK_SIZE - 1)) as i32;
                let cy = origin_y + (gy.min(CHUNK_SIZE - 1)) as i32;
                manager.surface_at(cx, cy).unwrap_or((0, [0.0; 4]))
            });

            vertices.push(TerrainVertex {
                pos: Vec3::new(wx as f32, wy as f32, z as f32),
                material_weights: weights,
            });
        }
    }

    // Deux triangles par quad. Ordre CCW vu de dessus (+Z) pour ne pas être
    // éliminé par le back-face culling (la caméra regarde le terrain par-dessus).
    let mut indices = Vec::with_capacity(CHUNK_SIZE * CHUNK_SIZE * 6);
    let idx = |gx: usize, gy: usize| (gy * dim + gx) as u32;
    for gy in 0..CHUNK_SIZE {
        for gx in 0..CHUNK_SIZE {
            let a = idx(gx, gy); // (x,   y)
            let b = idx(gx + 1, gy); // (x+1, y)
            let c = idx(gx, gy + 1); // (x,   y+1)
            let d = idx(gx + 1, gy + 1); // (x+1, y+1)
            indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }

    (vertices, indices)
}
