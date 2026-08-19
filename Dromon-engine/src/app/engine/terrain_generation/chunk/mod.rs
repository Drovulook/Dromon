mod chunk_data;
mod chunk_store;
mod terrain_source;
mod voxel;

pub use chunk_store::{ChunkStore, TerrainSnapshot};
pub use terrain_source::{GenParams, TerrainSource};
pub use voxel::Voxel;

/// Côté horizontal d'un chunk en voxels (axes X et Y).
pub const CHUNK_SIZE: usize = 64;
/// Hauteur d'un chunk en voxels (axe Z — le monde est en Z-up).
pub const CHUNK_HEIGHT: usize = 256;

/// Densité de l'iso-surface : la surface est l'ensemble des points où
/// `density == ISO_LEVEL` (au-dessus = air, en dessous = matière). Le mailleur
/// interpole la densité **entre** les coins d'un cube → surface lisse, sans marches.
pub const ISO_LEVEL: f32 = 0.0;
