mod chunk_manager;
mod voxel;

pub use chunk_manager::{ChunkManager, GenParams};
pub use voxel::Voxel;

use glam::IVec2;

/// Côté horizontal d'un chunk en voxels (axes X et Y).
pub const CHUNK_SIZE: usize = 64;
/// Hauteur d'un chunk en voxels (axe Z — le monde est en Z-up).
pub const CHUNK_HEIGHT: usize = 256;

/// Densité de l'iso-surface : la surface est l'ensemble des points où
/// `density == ISO_LEVEL` (au-dessus = air, en dessous = matière). Le mailleur
/// interpole la densité **entre** les coins d'un cube → surface lisse, sans marches.
pub const ISO_LEVEL: f32 = 0.0;

/// Marqueur d'un chunk **chargé**. Volontairement minimal : le terrain n'est pas
/// stocké voxel par voxel — toute la densité est produite à la volée par le champ 3D
/// ([`super::generation::DensityField`]).
///
/// ⚠ Le niveau de LOD et le masque de coutures ont quitté cette structure : ils vivent
/// dans la [`super::lod::grid::LodGrid`], qui change à chaque déplacement de caméra
/// alors que le chunk, lui, ne bouge pas. Les séparer est ce qui permet de mailler sur
/// un thread de fond (le relief est immuable et partageable ; la config LOD voyage en
/// `Arc` avec chaque lot).
pub struct Chunk {
    /// Position du chunk dans la grille de chunks (coordonnées chunk, pas voxel).
    pub coord: IVec2,
}

impl Chunk {
    /// Crée un chunk : simple marqueur de région chargée.
    pub fn new(coord: IVec2) -> Chunk {
        Chunk { coord }
    }
}
