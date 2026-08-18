use glam::IVec2;
use rustc_hash::FxHashMap;

use crate::app::engine::renderer::render_resources::TerrainMesh;

/// Les meshes GPU du terrain, plus la liste **compacte** de leurs coordonnées.
///
/// Le doublon est délibéré : le frustum culling relit toutes les coordonnées à chaque
/// frame, et les itérer depuis la `HashMap` traverse ~4 Mo (une entrée pèse 248 octets)
/// pour n'en exploiter que 63 Ko. Le `Vec` tient en L2 et se parcourt linéairement.
#[derive(Default)]
pub struct TerrainMeshes {
    meshes: FxHashMap<IVec2, TerrainMesh>,
    coords: Vec<IVec2>,
}

impl TerrainMeshes {
    pub fn new() -> Self {
        Self {
            meshes: FxHashMap::default(),
            coords: Vec::new(),
        }
    }

    /// Coordonnées en mémoire contiguë — l'itération du culling passe par là.
    #[inline]
    pub fn coords(&self) -> &[IVec2] {
        &self.coords
    }

    #[inline]
    pub fn get(&self, coord: &IVec2) -> Option<&TerrainMesh> {
        self.meshes.get(coord)
    }

    pub fn values(&self) -> impl Iterator<Item = &TerrainMesh> {
        self.meshes.values()
    }

    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }

    /// Renvoie le mesh remplacé, le cas échéant. Un **remplacement** (cas courant du
    /// LOD) ne touche pas `coords` : la coordonnée y est déjà.
    pub fn insert(&mut self, coord: IVec2, mesh: TerrainMesh) -> Option<TerrainMesh> {
        let previous = self.meshes.insert(coord, mesh);
        if previous.is_none() {
            self.coords.push(coord);
        }
        previous
    }

    /// O(n) sur `coords`, mais n'arrive que pour un chunk devenu **vide** — rare.
    pub fn remove(&mut self, coord: &IVec2) -> Option<TerrainMesh> {
        let mesh = self.meshes.remove(coord)?;
        if let Some(i) = self.coords.iter().position(|c| c == coord) {
            self.coords.swap_remove(i);
        }
        Some(mesh)
    }
}
