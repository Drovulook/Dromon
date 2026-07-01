use super::chunk::CHUNK_SIZE;
use super::chunk_manager::ChunkManager;
use crate::app::engine::renderer::render_resources::TerrainVertex;
use crate::profile;
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

/// Rayon (en voxels) du stencil de différences servant à calculer les normales.
/// `1` = normale fidèle à chaque facette (net, mais sujet à l'aliasing de
/// normales : le « quadrillage » sur les pentes). Plus grand = normale lissée sur
/// une zone plus large → éclairage doux, au prix d'arêtes vives un peu adoucies.
const NORMAL_RADIUS: i32 = 5;

/// Construit le mesh de surface d'un chunk en coordonnées monde (`model` =
/// identité au draw).
pub fn mesh_chunk(manager: &ChunkManager, coord: IVec2) -> (Vec<TerrainVertex>, Vec<u32>) {
    profile!();
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

            // Normale lissée. Pour un champ de hauteur z = f(x, y), la normale
            // (orientée +Z) est `(-∂f/∂x, -∂f/∂y, 1)` normalisé. On estime la pente
            // par différences centrées, mais en MOYENNANT plusieurs rayons
            // `k = 1..=NORMAL_RADIUS` : la normale représente alors la pente MACRO
            // et ignore le micro-jitter du bruit à l'échelle du voxel → fini le
            // quadrillage d'aliasing de normales, sans aplatir la géométrie.
            //
            // Pourquoi moyenner plusieurs rayons (et pas juste un pas large) ? Une
            // différence centrée de pas 1 ne relie que les voxels de parité OPPOSÉE
            // au centre, un pas 2 que ceux de MÊME parité : dans les deux cas le
            // réseau se scinde en deux sous-grilles découplées → c'est ce qui
            // créait l'ancien « damier diagonal ». En mélangeant pas pairs et
            // impairs, on recouple les sous-grilles : pas de damier.
            let r = NORMAL_RADIUS.max(1);
            let mut dzdx = 0.0f32;
            let mut dzdy = 0.0f32;
            for k in 1..=r {
                let s = (2 * k) as f32; // dénominateur de la différence centrée
                dzdx += (height(wx + k, wy) - height(wx - k, wy)) / s;
                dzdy += (height(wx, wy + k) - height(wx, wy - k)) / s;
            }
            let inv = 1.0 / r as f32;
            let normal = Vec3::new(-dzdx * inv, -dzdy * inv, 1.0).normalize();

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
    //
    // Triangulation en DAMIER : on alterne la diagonale de découpe d'une cellule
    // à l'autre. Si toutes les cellules étaient coupées dans le même sens
    // (diagonale `a–c`), la surface acquiert un « grain » directionnel — des
    // chevrons réguliers le long de cette diagonale, visibles en lumière rasante
    // (c'est le quadrillage géométrique résiduel). En alternant `a–c` / `b–d`
    // selon la parité de la cellule, ce grain se brouille.
    //
    // La parité est calculée en coordonnées MONDE (`wx_cell + wy_cell`) et non
    // locales : ainsi le motif est continu d'un chunk à l'autre, sans rupture du
    // damier sur les coutures. (`rem_euclid(2)` car les coords monde peuvent être
    // négatives.)
    let mut indices: Vec<u32> = Vec::with_capacity((dim - 1) * (dim - 1) * 6);
    for lx in 0..dim - 1 {
        for ly in 0..dim - 1 {
            let a = grid(lx, ly) as u32;
            let b = grid(lx + 1, ly) as u32;
            let c = grid(lx + 1, ly + 1) as u32;
            let d = grid(lx, ly + 1) as u32;

            let wx_cell = origin_x + lx as i32;
            let wy_cell = origin_y + ly as i32;
            if (wx_cell + wy_cell).rem_euclid(2) == 0 {
                // Diagonale a–c (du coin bas-gauche au coin haut-droit).
                indices.extend_from_slice(&[a, b, c, a, c, d]);
            } else {
                // Diagonale b–d (du coin bas-droit au coin haut-gauche).
                indices.extend_from_slice(&[a, b, d, b, c, d]);
            }
        }
    }

    (vertices, indices)
}
