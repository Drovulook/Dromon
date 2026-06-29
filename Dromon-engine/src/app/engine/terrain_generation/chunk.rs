use super::chunk_manager::MATERIAL_AIR;
use glam::IVec2;

/// Côté horizontal d'un chunk en voxels (axes X et Y).
pub const CHUNK_SIZE: usize = 32;
/// Hauteur d'un chunk en voxels (axe Z — le monde est en Z-up).
pub const CHUNK_HEIGHT: usize = 256;

/// Un voxel : jusqu'à 4 matériaux dominants mélangés (« top-K »).
///
/// `materials` contient les IDs ; `weights` leurs proportions quantifiées sur
/// `[0, 255]` (somme attendue = 255 pour un voxel plein). L'air est le cas
/// particulier où tous les poids sont nuls — voir [`Voxel::AIR`].
///
/// 12 octets, contre 16 pour un `Vec4<f32>`, et bien plus expressif : on peut
/// avoir des centaines de matériaux dans le jeu tout en n'en mélangeant que 4
/// par voxel. Les poids `f32` du `TerrainVertex` (côté GPU) sont dérivés de
/// ceux-ci au moment du meshing.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Voxel {
    pub materials: [u16; 4],
    pub weights: [u8; 4],
}

impl Voxel {
    /// Voxel vide. C'est aussi la valeur par défaut d'un chunk fraîchement créé.
    pub const AIR: Voxel = Voxel {
        materials: [MATERIAL_AIR; 4],
        weights: [0; 4],
    };

    /// Voxel composé d'un seul matériau (poids plein sur le premier canal).
    pub fn solid(material: u16) -> Voxel {
        Voxel {
            materials: [material, MATERIAL_AIR, MATERIAL_AIR, MATERIAL_AIR],
            weights: [255, 0, 0, 0],
        }
    }

    /// `true` si le voxel ne contient aucune matière.
    pub fn is_air(&self) -> bool {
        self.weights == [0; 4]
    }
}

impl Default for Voxel {
    fn default() -> Self {
        Voxel::AIR
    }
}

/// Une grille 3D dense de voxels couvrant `CHUNK_SIZE × CHUNK_SIZE × CHUNK_HEIGHT`.
///
/// Stockage volontairement encapsulé derrière [`Chunk::get_voxel`] /
/// [`Chunk::set_voxel`] : aujourd'hui c'est un tableau plat dense, mais on
/// pourra passer à une représentation compressée (palette + RLE par colonne)
/// sans toucher au reste du moteur.
pub struct Chunk {
    /// Position du chunk dans la grille de chunks (coordonnées chunk, pas voxel).
    pub coord: IVec2,
    /// Voxels en tableau plat, **Z contigu** : les voxels d'une même colonne
    /// `(x, y)` sont adjacents en mémoire, ce qui rend le scan vertical
    /// (recherche de surface, meshing) cache-friendly.
    voxels: Box<[Voxel]>,
}

impl Chunk {
    /// Crée un chunk entièrement constitué d'air.
    pub fn new(coord: IVec2) -> Chunk {
        let voxels = vec![Voxel::AIR; CHUNK_SIZE * CHUNK_SIZE * CHUNK_HEIGHT].into_boxed_slice();
        Chunk { coord, voxels }
    }

    /// Index plat d'un voxel local. Z varie le plus vite (colonne contiguë).
    #[inline]
    fn index(x: usize, y: usize, z: usize) -> usize {
        debug_assert!(x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_HEIGHT);
        (x * CHUNK_SIZE + y) * CHUNK_HEIGHT + z
    }

    /// Voxel aux coordonnées **locales** au chunk.
    pub fn get_voxel(&self, x: usize, y: usize, z: usize) -> Voxel {
        self.voxels[Self::index(x, y, z)]
    }

    /// Remplace le voxel aux coordonnées **locales** au chunk.
    pub fn set_voxel(&mut self, x: usize, y: usize, z: usize, voxel: Voxel) {
        self.voxels[Self::index(x, y, z)] = voxel;
    }

    /// Z du voxel non-air le plus haut d'une colonne, ou `None` si la colonne
    /// est entièrement vide. C'est la « hauteur de surface » utilisée par le
    /// meshing.
    pub fn surface_height(&self, x: usize, y: usize) -> Option<usize> {
        (0..CHUNK_HEIGHT)
            .rev()
            .find(|&z| !self.get_voxel(x, y, z).is_air())
    }
}
