mod chunk_manager;
mod voxel;

pub use chunk_manager::{ChunkManager, GenParams};
pub use voxel::Voxel;

use super::lod::TransitionFaces;
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
/// ([`super::density_field::DensityField`]).
/// [`super::chunk_manager`].
pub struct Chunk {
    /// Position du chunk dans la grille de chunks (coordonnées chunk, pas voxel).
    pub coord: IVec2,
    /// Niveau de détail : pas d'échantillonnage du champ = `1 << lod_level`
    /// (0 = pleine réso, 1 = ÷4 sommets, 2 = ÷16). Stocké **sur le chunk** (et pas
    /// seulement passé en argument) pour préparer le LOD dynamique : re-mailler un
    /// chunk quand son niveau change. Source de vérité lue par le mailleur.
    pub lod_level: u8,
    /// Faces bordant un voisin **plus grossier** ⇒ portant une cellule de transition
    /// Transvoxel (cf. [`super::lod::transition_faces`]). Cache **dérivé** du LOD des
    /// voisins — pas une source de vérité : recalculable à tout moment depuis les LOD.
    /// À rafraîchir après toute (ré)assignation de LOD via
    /// [`super::chunk_manager::ChunkManager::refresh_transition_faces`] (le masque
    /// dépend des voisins, pas seulement de soi).
    pub transition_faces: TransitionFaces,
}

impl Chunk {
    /// Crée un chunk (simple marqueur de région chargée), en pleine résolution et sans
    /// face de transition (masque rempli plus tard, une fois les LOD des voisins connus).
    pub fn new(coord: IVec2) -> Chunk {
        Chunk {
            coord,
            lod_level: 0,
            transition_faces: TransitionFaces::default(),
        }
    }
}
