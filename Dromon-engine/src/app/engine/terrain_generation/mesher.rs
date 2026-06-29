use super::chunk::CHUNK_SIZE;
use super::chunk_manager::ChunkManager;
use crate::app::engine::renderer::render_resources::TerrainVertex;
use glam::{IVec2, Vec3};

// ─── Mailleur heightfield ────────────────────────────────────────────────────
//
// Un sommet de mesh par **colonne de voxel** (x, y) — même résolution que la
// grille de voxels — relié en une nappe de triangles. La seule, mais décisive,
// différence avec le tout premier mailleur : la hauteur d'un sommet est le
// **flottant** du bruit de Perlin (`column_height`), pas la hauteur entière du
// voxel le plus haut. C'est ce qui supprime les marches : la surface suit la
// vraie courbe continue du bruit, échantillonnée à chaque voxel.
//
// Le terrain est ici un pur *champ de hauteur* (une surface par colonne, pas de
// surplombs). Le champ de densité (`Chunk`) reste le modèle éditable ; le jour
// où l'on creuse des grottes, on re-maillera les chunks édités avec un
// extracteur volumétrique (Surface Nets / Marching Cubes) qui lit la densité 3D.
//
// Couture sans fissure : hauteur et normales ne dépendent que de la position
// MONDE, donc deux chunks voisins calculent des valeurs identiques sur leur
// colonne partagée. On maille les sommets `[0, CHUNK_SIZE]` **inclus** (la
// colonne de débordement est exactement la 1re colonne du chunk d'à côté).

/// Construit le mesh de surface d'un chunk en coordonnées monde (`model` =
/// identité au draw).
pub fn mesh_chunk(manager: &ChunkManager, coord: IVec2) -> (Vec<TerrainVertex>, Vec<u32>) {
    let n = CHUNK_SIZE as i32;
    let origin_x = coord.x * n;
    let origin_y = coord.y * n;

    // Sommets par côté : un par colonne de voxel + 1 colonne de débordement
    // partagée avec le voisin → la grille des deux chunks coïncide bord à bord.
    let dim = CHUNK_SIZE + 1;
    let grid = |lx: usize, ly: usize| lx * dim + ly;
    let height = |wx: i32, wy: i32| manager.column_height(wx, wy);

    // ── Sommets : un par colonne, à la hauteur flottante du bruit ───────────
    let mut vertices: Vec<TerrainVertex> = Vec::with_capacity(dim * dim);
    for lx in 0..dim {
        for ly in 0..dim {
            let wx = origin_x + lx as i32;
            let wy = origin_y + ly as i32;
            let h = height(wx, wy);
            let p = Vec3::new(wx as f32, wy as f32, h);

            // Normale = moyenne (pondérée par l'aire) des normales des 6 triangles
            // qui entourent réellement ce sommet dans la triangulation. Calculée à
            // partir des hauteurs voisines (toutes via `column_height` → fonction
            // des seules coordonnées MONDE, donc identique d'un chunk à l'autre :
            // pas de couture). Surtout, ces normales sont COHÉRENTES avec la
            // géométrie réelle des facettes — contrairement à un gradient par
            // différences centrées, dont le décalage créait le damier diagonal.
            // Les 6 triangles correspondent au découpage de quad `a–c` utilisé
            // plus bas (voisins E, W, N, S, NE, SW).
            let pt = |dx: i32, dy: i32| {
                let (x, y) = (wx + dx, wy + dy);
                Vec3::new(x as f32, y as f32, height(x, y))
            };
            let (e, w, north, south, ne, sw) =
                (pt(1, 0), pt(-1, 0), pt(0, 1), pt(0, -1), pt(1, 1), pt(-1, -1));
            // Produit vectoriel non normalisé = normale de facette pondérée par
            // l'aire ; même enroulement que l'émission des triangles → +Z.
            let face = |a: Vec3, b: Vec3, c: Vec3| (b - a).cross(c - a);
            let normal = (face(p, e, ne)
                + face(p, ne, north)
                + face(w, p, north)
                + face(south, e, p)
                + face(sw, south, p)
                + face(sw, p, w))
            .normalize();

            // Couleur du voxel plein juste sous la surface (mélange des matériaux
            // dominants → déjà résolue côté CPU, le shader n'a plus qu'à l'afficher).
            let color = manager.color_at(wx, wy, h.floor() as i32);

            vertices.push(TerrainVertex {
                pos: p,
                normal,
                color,
            });
        }
    }

    // ── Triangles : 2 par cellule de la grille ─────────────────────────────
    // Enroulement choisi pour que la normale géométrique pointe vers +Z (face du
    // dessus visible), conforme au front-face CCW du pipeline.
    let mut indices: Vec<u32> = Vec::with_capacity((dim - 1) * (dim - 1) * 6);
    for lx in 0..dim - 1 {
        for ly in 0..dim - 1 {
            let a = grid(lx, ly) as u32;
            let b = grid(lx + 1, ly) as u32;
            let c = grid(lx + 1, ly + 1) as u32;
            let d = grid(lx, ly + 1) as u32;
            indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }

    (vertices, indices)
}
