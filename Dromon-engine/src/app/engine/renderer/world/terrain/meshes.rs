use bytemuck::{Pod, Zeroable};
use glam::IVec2;
use rustc_hash::FxHashMap;

use crate::app::engine::renderer::render_resources::TerrainMesh;
use crate::app::engine::terrain_generation::{CHUNK_SIZE, ShownLods};

/// Couture d'un chunk, poussée à chaque draw. ⚠ Même disposition que `ChunkSeam`
/// (`lib/chunk_seam.slang`).
///
/// Dépend de ce que les voisins **affichent** : un voisin qui change de LOD ne modifie que
/// ces 16 octets, pas le mesh.
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
    /// Couture du chunk `coord` d'après les LOD affichés.
    pub fn new(shown: &ShownLods, coord: IVec2) -> ChunkSeam {
        ChunkSeam {
            origin: coord * CHUNK_SIZE as i32,
            lod_step: (1u32 << shown.get(coord).unwrap_or(0)) as f32,
            bits: shown.seam_bits(coord),
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
    /// Rang dans [`InstalledChunks::coords`], pour un retrait sans parcours.
    index: usize,
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
        let index = self.chunks.get(&coord).map_or(self.coords.len(), |c| c.index);
        let previous = self.chunks.insert(coord, InstalledChunk { mesh, seam, index });
        if previous.is_none() {
            self.coords.push(coord);
        }
        previous.map(|c| c.mesh)
    }

    /// Remplace la couture d'un chunk installé, sans effet s'il ne l'est pas.
    pub fn set_seam(&mut self, coord: IVec2, seam: ChunkSeam) {
        if let Some(chunk) = self.chunks.get_mut(&coord) {
            chunk.seam = seam;
        }
    }

    /// `swap_remove` met le dernier chunk de `coords` à la place libérée : on corrige son rang.
    pub fn remove(&mut self, coord: &IVec2) -> Option<TerrainMesh> {
        let chunk = self.chunks.remove(coord)?;
        self.coords.swap_remove(chunk.index);
        if let Some(moved) = self.coords.get(chunk.index)
            && let Some(moved) = self.chunks.get_mut(moved)
        {
            moved.index = chunk.index;
        }
        Some(chunk.mesh)
    }
}
