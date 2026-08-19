use glam::IVec2;
use rustc_hash::FxHashMap;

use crate::app::engine::renderer::render_resources::TerrainMesh;

/// Tout ce qu'un chunk **chargé** possède côté rendu. Purement *dérivé* : reconstructible
/// depuis `(graine, coord, LOD)`, donc jetable sans autre forme de procès au déchargement.
/// Les données du joueur, elles, vivent dans le
/// [`ChunkStore`](crate::app::engine::terrain_generation::ChunkStore) et lui survivent.
///
/// N'a qu'un champ aujourd'hui ; c'est ici que viendront les instances de props
/// (arbres, rochers) et la géométrie de collision, qui suivent exactement le même
/// cycle de vie.
pub struct LoadedChunk {
    pub mesh: TerrainMesh,
}

/// Les chunks chargés, plus la liste **compacte** de leurs coordonnées.
///
/// Un chunk **vide** (aucune surface à mailler) n'a simplement pas d'entrée : Vulkan
/// interdit un buffer de taille 0.
///
/// Le doublon `coords` est délibéré : le frustum culling relit toutes les coordonnées à
/// chaque frame, et les itérer depuis la `HashMap` traverse ~4 Mo (une entrée pèse 248
/// octets) pour n'en exploiter que 63 Ko. Le `Vec` tient en L2 et se parcourt linéairement.
#[derive(Default)]
pub struct LoadedChunks {
    chunks: FxHashMap<IVec2, LoadedChunk>,
    coords: Vec<IVec2>,
}

impl LoadedChunks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Coordonnées en mémoire contiguë — l'itération du culling passe par là.
    #[inline]
    pub fn coords(&self) -> &[IVec2] {
        &self.coords
    }

    #[inline]
    pub fn get(&self, coord: &IVec2) -> Option<&LoadedChunk> {
        self.chunks.get(coord)
    }

    pub fn values(&self) -> impl Iterator<Item = &LoadedChunk> {
        self.chunks.values()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Renvoie le mesh remplacé, le cas échéant. Un **remplacement** (cas courant du
    /// LOD) ne touche pas `coords` : la coordonnée y est déjà.
    pub fn insert(&mut self, coord: IVec2, mesh: TerrainMesh) -> Option<TerrainMesh> {
        let previous = self.chunks.insert(coord, LoadedChunk { mesh });
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
