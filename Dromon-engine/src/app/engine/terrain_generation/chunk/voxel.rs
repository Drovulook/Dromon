use crate::app::engine::terrain_generation::generation::material::MATERIAL_AIR;

/// Matériaux gardés par point. 3 suffisent (un point est rarement à cheval sur plus
/// de 3 frontières), et le fragment shader lira 3 sommets × 3 matériaux.
pub const VOXEL_MATERIALS: usize = 3;

/// Composition d'un point de terrain : jusqu'à [`VOXEL_MATERIALS`] **matériaux**
/// dominants mélangés, poids de somme 255. Produit par le pipeline de matériaux (cf.
/// `generation::material::evaluate`), porté par les sommets du terrain.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Voxel {
    pub materials: [u16; VOXEL_MATERIALS],
    pub weights: [u8; VOXEL_MATERIALS],
}

impl Voxel {
    /// Descripteur « air » (aucun matériau, poids nuls).
    pub const AIR: Voxel = Voxel {
        materials: [MATERIAL_AIR; VOXEL_MATERIALS],
        weights: [0; VOXEL_MATERIALS],
    };

    /// Un seul matériau, à 100 %.
    pub fn solid(material: u16) -> Voxel {
        let mut voxel = Voxel::AIR;
        voxel.materials[0] = material;
        voxel.weights[0] = 255;
        voxel
    }
}

impl Default for Voxel {
    fn default() -> Self {
        Voxel::AIR
    }
}
