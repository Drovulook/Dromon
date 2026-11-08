//! Un gros buffer device-local partagé par de nombreux meshes : installer un mesh,
//! c'est réserver une tranche (arithmétique), pas créer un objet Vulkan.

use super::suballocator::Suballocator;
use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

/// Tranche d'une [`Arena`], en éléments. Ni `Copy` ni `Clone` : [`Arena::free`] la
/// consomme, donc une double libération ne compile pas.
#[derive(Debug)]
pub struct ArenaSlice {
    /// Premier élément : `firstIndex` ou `vertexOffset` du draw.
    pub first: u32,
    pub count: u32,
}

/// Occupation d'une arène, en éléments.
pub struct ArenaUsage {
    pub capacity: u64,
    pub used: u64,
    pub holes: usize,
    /// Plus grande tranche encore allouable : sous la taille d'un gros mesh, l'arène
    /// est trop fragmentée.
    pub largest_free: u64,
}

pub struct Arena {
    buffer: Buffer,
    element_size: u64,
    ranges: Suballocator,
}

impl Arena {
    /// `capacity` éléments de `element_size` octets. `usage` : rôle du buffer
    /// (`VERTEX_BUFFER`, `INDEX_BUFFER`…) ; `TRANSFER_DST` est ajouté pour les envois.
    pub fn new(
        context: Arc<RenderingContext>,
        element_size: u64,
        capacity: u32,
        usage: vk::BufferUsageFlags,
    ) -> Result<Self> {
        let buffer = Buffer::new(
            context,
            element_size * capacity as u64,
            usage | vk::BufferUsageFlags::TRANSFER_DST,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;
        Ok(Self {
            buffer,
            element_size,
            ranges: Suballocator::new(capacity as u64),
        })
    }

    pub fn buffer(&self) -> vk::Buffer {
        self.buffer.buffer
    }

    /// Décalage en octets d'une tranche, pour les copies (`dst_offset`).
    pub fn byte_offset(&self, slice: &ArenaSlice) -> u64 {
        slice.first as u64 * self.element_size
    }

    /// `None` si aucun trou ne tient `count` éléments d'un seul tenant.
    pub fn allocate(&mut self, count: u32) -> Option<ArenaSlice> {
        // Alignement 1 : les offsets sont en éléments, donc toujours alignés en octets.
        let first = self.ranges.try_alloc(count as u64, 1)?;
        Some(ArenaSlice {
            first: first as u32,
            count,
        })
    }

    /// Rend la tranche. ⚠ Le GPU ne doit plus la lire : l'appelant attend la fin des
    /// frames en vol avant d'appeler.
    pub fn free(&mut self, slice: ArenaSlice) {
        self.ranges.dealloc(slice.first as u64, slice.count as u64);
    }

    pub fn usage(&self) -> ArenaUsage {
        let capacity = self.ranges.capacity();
        ArenaUsage {
            capacity,
            used: capacity - self.ranges.free_total(),
            holes: self.ranges.holes(),
            largest_free: self.ranges.largest_free(),
        }
    }
}
