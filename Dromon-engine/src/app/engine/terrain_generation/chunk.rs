use super::material::MATERIAL_AIR;
use glam::IVec2;

/// Côté horizontal d'un chunk en voxels (axes X et Y).
pub const CHUNK_SIZE: usize = 64;
/// Hauteur d'un chunk en voxels (axe Z — le monde est en Z-up).
pub const CHUNK_HEIGHT: usize = 256;

/// Densité de l'iso-surface : la surface est l'ensemble des points où
/// `density == ISO_LEVEL` (au-dessus = air, en dessous = matière). Le mailleur
/// interpole la densité **entre** les coins d'un cube → surface lisse, sans marches.
pub const ISO_LEVEL: f32 = 0.0;

/// Composition d'un point de terrain : jusqu'à 4 **matériaux** dominants mélangés.
/// Sert au choix du matériau de surface (cf. [`super::material::classify_solid`])
/// et au calcul de couleur au maillage.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Voxel {
    pub materials: [u16; 4],
    pub weights: [u8; 4],
}

impl Voxel {
    /// Descripteur « air » (aucun matériau, poids nuls).
    pub const AIR: Voxel = Voxel {
        materials: [MATERIAL_AIR; 4],
        weights: [0; 4],
    };

    /// Matériau unique (poids plein sur le premier canal).
    pub fn solid(material: u16) -> Voxel {
        Voxel {
            materials: [material, MATERIAL_AIR, MATERIAL_AIR, MATERIAL_AIR],
            weights: [255, 0, 0, 0],
        }
    }
}

impl Default for Voxel {
    fn default() -> Self {
        Voxel::AIR
    }
}

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
}

impl Chunk {
    /// Crée un chunk (simple marqueur de région chargée), en pleine résolution.
    pub fn new(coord: IVec2) -> Chunk {
        Chunk {
            coord,
            lod_level: 0,
        }
    }
}
