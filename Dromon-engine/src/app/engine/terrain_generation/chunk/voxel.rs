use crate::app::engine::terrain_generation::generation::material::MATERIAL_AIR;

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
