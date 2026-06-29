use super::chunk_manager::MATERIAL_AIR;
use glam::IVec2;

/// Côté horizontal d'un chunk en voxels (axes X et Y).
pub const CHUNK_SIZE: usize = 32;
/// Hauteur d'un chunk en voxels (axe Z — le monde est en Z-up).
pub const CHUNK_HEIGHT: usize = 256;

/// Valeur de densité de l'iso-surface : la surface du terrain est l'ensemble des
/// points où `density == ISO_LEVEL`. Au-dessus (densité < ISO) = air, en dessous
/// (densité > ISO) = matière. Le mailleur (Surface Nets) reconstruit cette
/// surface lisse en interpolant la densité **entre** les voxels — d'où l'absence
/// de marches, contrairement à un meshing « voxel plein le plus haut ».
pub const ISO_LEVEL: f32 = 0.0;

/// Un voxel : une **densité** signée + jusqu'à 4 matériaux dominants mélangés.
///
/// `density` situe le voxel par rapport à la surface (cf. [`ISO_LEVEL`]). C'est
/// le champ scalaire que Surface Nets échantillonne ; le modifier (creuser,
/// remblayer) déplace la surface en douceur. `materials`/`weights` portent la
/// composition (IDs + proportions quantifiées sur `[0, 255]`) pour le futur
/// texture splatting.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Voxel {
    pub density: f32,
    pub materials: [u16; 4],
    pub weights: [u8; 4],
}

impl Voxel {
    /// Voxel d'air « franc ». C'est la valeur par défaut d'un chunk fraîchement
    /// créé ; la génération écrase ensuite la densité par la vraie valeur lisse.
    pub const AIR: Voxel = Voxel {
        density: -1.0,
        materials: [MATERIAL_AIR; 4],
        weights: [0; 4],
    };

    /// Voxel composé d'un seul matériau, à la densité donnée (poids plein sur le
    /// premier canal).
    pub fn solid(density: f32, material: u16) -> Voxel {
        Voxel {
            density,
            materials: [material, MATERIAL_AIR, MATERIAL_AIR, MATERIAL_AIR],
            weights: [255, 0, 0, 0],
        }
    }

    /// `true` si le voxel est de l'air (densité sous l'iso-surface).
    pub fn is_air(&self) -> bool {
        self.density < ISO_LEVEL
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

    /// Densité du voxel aux coordonnées **locales** (raccourci pour le mailleur,
    /// qui ne lit que ce champ).
    pub fn density(&self, x: usize, y: usize, z: usize) -> f32 {
        self.voxels[Self::index(x, y, z)].density
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
