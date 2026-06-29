use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use std::mem::offset_of;
use std::sync::Arc;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct TerrainVertex {
    pub pos: Vec3,
    pub material_weights: [f32; 4], // poids des 4 matériaux dominants (top-K); pas de Vec4 car
                      // problème de padding sinon
}

impl TerrainVertex {
    pub fn get_binding_description() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<TerrainVertex>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    pub fn get_attribute_descriptions() -> [vk::VertexInputAttributeDescription; 2] {
        [
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 0,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: offset_of!(TerrainVertex, pos) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 1,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: offset_of!(TerrainVertex, material_weights) as u32,
            },
        ]
    }
}

/// Le mesh d'un chunk de terrain, côté GPU. Même structure que `ObjectMesh`
/// (staging host-visible + buffer device-local), mais sur des `TerrainVertex`.
pub struct TerrainMesh {
    context: Arc<RenderingContext>,

    pub vertices: Vec<TerrainVertex>,
    pub vertex_staging_buffer: Buffer,
    pub vertex_buffer: Buffer,

    pub indices: Vec<u32>,
    pub index_staging_buffer: Buffer,
    pub index_buffer: Buffer,
}

impl TerrainMesh {
    /// Alloue les buffers et copie les données dans le staging (host-visible).
    /// L'upload vers le device se fait plus tard via [`TerrainMesh::initialize`]
    /// (dans la passe de transfert au démarrage).
    pub fn new(
        context: Arc<RenderingContext>,
        vertices: Vec<TerrainVertex>,
        indices: Vec<u32>,
    ) -> Result<TerrainMesh> {
        let (vertex_staging_buffer, vertex_buffer) =
            Self::staging_and_device(&context, &vertices, vk::BufferUsageFlags::VERTEX_BUFFER)?;
        let (index_staging_buffer, index_buffer) =
            Self::staging_and_device(&context, &indices, vk::BufferUsageFlags::INDEX_BUFFER)?;

        Ok(TerrainMesh {
            context,
            vertices,
            vertex_staging_buffer,
            vertex_buffer,
            indices,
            index_staging_buffer,
            index_buffer,
        })
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
        staging.map_and_unmap(data)?;

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
                vk::IndexType::UINT32,
            );
        }
    }

    /// Copie staging → device. À appeler dans la passe de transfert initiale.
    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) {
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
