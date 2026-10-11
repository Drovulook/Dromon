use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use std::mem::offset_of;
use std::sync::Arc;

/// Nombre de cellules par côté de la grille d'un nœud d'eau.
pub const WATER_GRID_RESOLUTION: u32 = 32;

/// Sommet de la grille : ses coordonnées entières `(i, j)` dans la grille, stockées en
/// `f32`. La position monde est calculée dans le vertex shader (origine + taille du nœud).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct WaterVertex {
    pub grid_pos: Vec2,
}

impl WaterVertex {
    pub fn get_binding_description() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<WaterVertex>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    pub fn get_attribute_descriptions() -> [vk::VertexInputAttributeDescription; 1] {
        [vk::VertexInputAttributeDescription {
            binding: 0,
            location: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: offset_of!(WaterVertex, grid_pos) as u32,
        }]
    }
}

/// La grille **unique** partagée par tous les nœuds d'eau (CDLOD) : envoyée une fois au
/// démarrage, redessinée par nœud avec une position et une taille différentes.
/// Même structure que `TerrainMesh` (staging host-visible + buffer device-local).
pub struct WaterMesh {
    context: Arc<RenderingContext>,
    vertices: Vec<WaterVertex>,
    // u16 : (32 + 1)² = 1 089 sommets, bien en dessous de 65 536.
    indices: Vec<u16>,

    pub vertex_staging_buffer: Buffer,
    pub vertex_buffer: Buffer,

    pub index_staging_buffer: Buffer,
    pub index_buffer: Buffer,
}

impl WaterMesh {
    /// Construit la grille et la copie dans le staging. L'upload vers le device se fait
    /// via [`WaterMesh::record_upload`], dans la passe de transfert du démarrage.
    pub fn new(context: Arc<RenderingContext>) -> Result<WaterMesh> {
        let (vertices, indices) = Self::build_grid(WATER_GRID_RESOLUTION);
        let (vertex_staging_buffer, vertex_buffer) = Self::staging_and_device(
            &context,
            &vertices,
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;
        let (index_staging_buffer, index_buffer) =
            Self::staging_and_device(&context, &indices, vk::BufferUsageFlags::INDEX_BUFFER)?;

        Ok(WaterMesh {
            context,
            vertices,
            indices,
            vertex_staging_buffer,
            vertex_buffer,
            index_staging_buffer,
            index_buffer,
        })
    }

    /// Grille de `n × n` cellules, deux triangles par cellule.
    fn build_grid(n: u32) -> (Vec<WaterVertex>, Vec<u16>) {
        let side = n + 1;
        let mut vertices = Vec::with_capacity((side * side) as usize);
        for j in 0..side {
            for i in 0..side {
                vertices.push(WaterVertex {
                    grid_pos: Vec2::new(i as f32, j as f32),
                });
            }
        }

        let mut indices = Vec::with_capacity((n * n * 6) as usize);
        for j in 0..n {
            for i in 0..n {
                let a = (j * side + i) as u16; // (i, j)
                let b = a + 1; // (i+1, j)
                let c = a + side as u16; // (i, j+1)
                let d = c + 1; // (i+1, j+1)
                // Sens anti-horaire vu de dessus (+Z).
                indices.extend_from_slice(&[a, b, d, a, d, c]);
            }
        }
        (vertices, indices)
    }

    /// Nombre d'index à dessiner.
    #[inline]
    pub fn index_count(&self) -> u32 {
        self.indices.len() as u32
    }

    /// Crée la paire (buffer de staging host-visible, buffer device-local) et y
    /// copie `data`. `role` est l'usage final (VERTEX_BUFFER ou INDEX_BUFFER).
    fn staging_and_device<T: Copy>(
        context: &Arc<RenderingContext>,
        data: &[T],
        role: vk::BufferUsageFlags,
    ) -> Result<(Buffer, Buffer)> {
        let size = std::mem::size_of_val(data) as u64;
        let staging = Buffer::new(
            context.clone(),
            size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        staging.upload(data)?;

        let device = Buffer::new(
            context.clone(),
            size,
            vk::BufferUsageFlags::TRANSFER_DST | role,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;
        Ok((staging, device))
    }

    pub fn bind(&self, command_buffer: vk::CommandBuffer) {
        unsafe {
            self.context.device.cmd_bind_vertex_buffers2(
                command_buffer,
                0,
                &[self.vertex_buffer.buffer],
                &[0],
                None,
                None,
            );
            self.context.device.cmd_bind_index_buffer(
                command_buffer,
                self.index_buffer.buffer,
                0,
                vk::IndexType::UINT16,
            );
        }
    }

    /// Enregistre la copie staging → device. Appelée une seule fois, dans la passe de
    /// transfert initiale (suivie d'une attente complète de la queue : pas de barrière).
    pub fn record_upload(&self, command_buffer: &vk::CommandBuffer) {
        unsafe {
            self.context.device.cmd_copy_buffer(
                *command_buffer,
                self.vertex_staging_buffer.buffer,
                self.vertex_buffer.buffer,
                &[vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: std::mem::size_of_val(self.vertices.as_slice()) as u64,
                }],
            );
            self.context.device.cmd_copy_buffer(
                *command_buffer,
                self.index_staging_buffer.buffer,
                self.index_buffer.buffer,
                &[vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: std::mem::size_of_val(self.indices.as_slice()) as u64,
                }],
            );
        }
    }
}
