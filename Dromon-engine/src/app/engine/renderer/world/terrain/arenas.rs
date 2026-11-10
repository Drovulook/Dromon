//! Les arènes de géométrie du terrain : un vertex buffer et un index buffer partagés
//! par tous les chunks (cf. vault, Architecture/Batching.md).

use crate::app::engine::renderer::gpu_memory::arena::Arena;
use crate::app::engine::renderer::render_resources::TerrainVertex;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

// Tailles des arènes. Rayon 125 en mouvement : ~29 M sommets, ~5 index par sommet.
// Visé : ~50 % d'occupation, au-delà le first-fit se fragmente vite (mesuré : 97 % → plus
// aucun trou d'un seul tenant).
const VERTEX_ARENA_BYTES: u64 = 2 << 30;
const INDEX_ARENA_BYTES: u64 = 1 << 30;

pub struct TerrainArenas {
    /// En `TerrainVertex`.
    pub vertices: Arena,
    /// En index `u32`.
    pub indices: Arena,
}

impl TerrainArenas {
    pub fn new(context: &Arc<RenderingContext>) -> Result<Self> {
        let vertex_size = std::mem::size_of::<TerrainVertex>() as u64;
        let index_size = std::mem::size_of::<u32>() as u64;
        Ok(Self {
            vertices: Arena::new(
                context.clone(),
                vertex_size,
                (VERTEX_ARENA_BYTES / vertex_size) as u32,
                vk::BufferUsageFlags::VERTEX_BUFFER,
            )?,
            indices: Arena::new(
                context.clone(),
                index_size,
                (INDEX_ARENA_BYTES / index_size) as u32,
                vk::BufferUsageFlags::INDEX_BUFFER,
            )?,
        })
    }

    /// Vide les deux arènes pour un terrain neuf, sans réallouer. ⚠ GPU à l'arrêt.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }
}
