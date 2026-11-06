use bytemuck::{Pod, Zeroable};
use glam::IVec2;
use rustc_hash::FxHashMap;

use crate::app::engine::renderer::render_resources::TerrainMesh;
use crate::app::engine::terrain_generation::{CHUNK_SIZE, LodGrid};

/// Couture d'un chunk, poussée à chaque draw. ⚠ Même disposition que `ChunkSeam`
/// (`lib/chunk_seam.slang`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ChunkSeam {
    /// Coin `(x, y)` du chunk, en voxels.
    pub origin: IVec2,
    /// Pas du chunk (`1 << lod`).
    pub lod_step: f32,
    /// Bits 0-3 : faces à dalle ; bits 4-11 : atténuations.
    pub bits: u32,
}

impl ChunkSeam {
    /// Couture du chunk `coord` sous la configuration `grid`.
    pub fn new(grid: &LodGrid, coord: IVec2) -> ChunkSeam {
        ChunkSeam {
            origin: coord * CHUNK_SIZE as i32,
            lod_step: (1u32 << grid.lod(coord)) as f32,
            bits: grid.faces(coord).bits() as u32 | (grid.taper(coord) as u32) << 4,
        }
    }

    /// La dalle de la face d'index `face` dans `Face::ALL` est-elle à dessiner ?
    #[inline]
    pub fn has_slab(&self, face: usize) -> bool {
        self.bits & (1 << face) != 0
    }
}

/// Tout ce qu'un chunk **installé** possède côté rendu. Purement *dérivé* : reconstructible
/// depuis `(graine, coord, LOD)`, donc jetable sans autre forme de procès au déchargement.
/// Les données du joueur, elles, vivent dans le
/// [`ChunkStore`](crate::app::engine::terrain_generation::ChunkStore) et lui survivent.
///
/// C'est ici que viendront les instances de props (arbres, rochers) et la géométrie de
/// collision, qui suivent exactement le même cycle de vie.
pub struct InstalledChunk {
    pub mesh: TerrainMesh,
    pub seam: ChunkSeam,
}

/// Les chunks **installés** (mesh présent sur le GPU), plus la liste **compacte** de
/// leurs coordonnées.
///
/// « Installé » ≠ « chargé » : un chunk chargé (présent dans la
/// [`LodGrid`](crate::app::engine::terrain_generation::LodGrid)) n'a pas d'entrée ici
/// tant que son mesh n'est pas bâti, ni jamais s'il est **vide** (aucune surface à
/// mailler) — Vulkan interdit un buffer de taille 0.
///
/// Le doublon `coords` est délibéré : le frustum culling relit toutes les coordonnées à
/// chaque frame, et les itérer depuis la `HashMap` traverse ~4 Mo (une entrée pèse 248
/// octets) pour n'en exploiter que 63 Ko. Le `Vec` tient en L2 et se parcourt linéairement.
#[derive(Default)]
pub struct InstalledChunks {
    chunks: FxHashMap<IVec2, InstalledChunk>,
    coords: Vec<IVec2>,
}

impl InstalledChunks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Coordonnées en mémoire contiguë — l'itération du culling passe par là.
    #[inline]
    pub fn coords(&self) -> &[IVec2] {
        &self.coords
    }

    #[inline]
    pub fn get(&self, coord: &IVec2) -> Option<&InstalledChunk> {
        self.chunks.get(coord)
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Renvoie le mesh remplacé, le cas échéant. Un **remplacement** (cas courant du
    /// LOD) ne touche pas `coords` : la coordonnée y est déjà.
    pub fn insert(
        &mut self,
        coord: IVec2,
        mesh: TerrainMesh,
        seam: ChunkSeam,
    ) -> Option<TerrainMesh> {
        let previous = self.chunks.insert(coord, InstalledChunk { mesh, seam });
        if previous.is_none() {
            self.coords.push(coord);
        }
        previous.map(|c| c.mesh)
    }

    /// O(n) sur `coords`, mais n'arrive que pour un chunk devenu **vide** ou déchargé.
    pub fn remove(&mut self, coord: &IVec2) -> Option<TerrainMesh> {
        let chunk = self.chunks.remove(coord)?;
        if let Some(i) = self.coords.iter().position(|c| c == coord) {
            self.coords.swap_remove(i);
        }
        Some(chunk.mesh)
    }
}
