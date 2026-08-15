use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;
use crate::profile;
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
    pub normal: Vec3, // normale lissée (gradient de densité), pour l'éclairage
    pub color: Vec3,  // couleur du sommet : mélange des matériaux résolu côté CPU
}

impl TerrainVertex {
    pub fn get_binding_description() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<TerrainVertex>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    pub fn get_attribute_descriptions() -> [vk::VertexInputAttributeDescription; 3] {
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
                format: vk::Format::R32G32B32_SFLOAT,
                offset: offset_of!(TerrainVertex, normal) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 2,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: offset_of!(TerrainVertex, color) as u32,
            },
        ]
    }
}

/// Géométrie d'un chunk **côté CPU**, telle que la produit le mailleur.
///
/// Toujours manipulée derrière un `Arc` : le même bloc est référencé à la fois par le
/// [`TerrainMesh`] affiché et par le cache multi-LOD, sans être dupliqué. Un chunk
/// re-maillé puis revisité ne repaie donc que l'upload GPU (~50 µs), pas le maillage
/// (2–10 ms).
pub struct MeshData {
    pub vertices: Vec<TerrainVertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Chunk sans géométrie (entièrement plein ou entièrement vide). Vulkan interdit les
    /// buffers de taille 0 : ces chunks n'ont pas de [`TerrainMesh`] du tout.
    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty() || self.indices.is_empty()
    }

    /// Empreinte RAM approximative, pour le budget du cache.
    pub fn byte_size(&self) -> usize {
        std::mem::size_of_val(self.vertices.as_slice())
            + std::mem::size_of_val(self.indices.as_slice())
    }
}

/// Le mesh d'un chunk de terrain, côté GPU. Même structure que `ObjectMesh`
/// (staging host-visible + buffer device-local), mais sur des `TerrainVertex`.
pub struct TerrainMesh {
    context: Arc<RenderingContext>,
    /// Géométrie source, partagée avec le cache multi-LOD.
    pub data: Arc<MeshData>,

    pub vertex_staging_buffer: Buffer,
    pub vertex_buffer: Buffer,

    pub index_staging_buffer: Buffer,
    pub index_buffer: Buffer,
}

impl TerrainMesh {
    /// Alloue les buffers et copie les données dans le staging (host-visible).
    /// L'upload vers le device se fait plus tard via [`TerrainMesh::record_upload`],
    /// dans la passe de transfert du démarrage ou en tête d'une frame.
    pub fn new(context: Arc<RenderingContext>, data: Arc<MeshData>) -> Result<TerrainMesh> {
        profile!();
        let (vertex_staging_buffer, vertex_buffer) = Self::staging_and_device(
            &context,
            &data.vertices,
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;
        let (index_staging_buffer, index_buffer) =
            Self::staging_and_device(&context, &data.indices, vk::BufferUsageFlags::INDEX_BUFFER)?;

        Ok(TerrainMesh {
            context,
            data,
            vertex_staging_buffer,
            vertex_buffer,
            index_staging_buffer,
            index_buffer,
        })
    }

    /// Nombre d'index à dessiner.
    #[inline]
    pub fn index_count(&self) -> u32 {
        self.data.indices.len() as u32
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
                vk::IndexType::UINT32,
            );
        }
    }

    /// Enregistre la copie staging → device. Utilisable dans la passe de transfert
    /// initiale **comme** en tête d'une frame de jeu (LOD dynamique) : dans ce second
    /// cas, l'appelant doit poser derrière une barrière
    /// `TRANSFER_WRITE → VERTEX_ATTRIBUTE_READ | INDEX_READ`
    /// (cf. `World::record_terrain_uploads`) — la passe initiale, elle, est suivie d'une
    /// attente complète de la queue.
    pub fn record_upload(&self, command_buffer: &vk::CommandBuffer) {
        unsafe {
            self.context.device.cmd_copy_buffer(
                *command_buffer,
                self.vertex_staging_buffer.buffer,
                self.vertex_buffer.buffer,
                &[vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: std::mem::size_of_val(self.data.vertices.as_slice()) as u64,
                }],
            );
            self.context.device.cmd_copy_buffer(
                *command_buffer,
                self.index_staging_buffer.buffer,
                self.index_buffer.buffer,
                &[vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: std::mem::size_of_val(self.data.indices.as_slice()) as u64,
                }],
            );
        }
    }
}
