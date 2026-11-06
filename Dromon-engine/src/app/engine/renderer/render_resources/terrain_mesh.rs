use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;
use crate::app::engine::terrain_generation::{VOXEL_MATERIALS, Voxel};
use crate::profile;
use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use std::mem::offset_of;
use std::ops::Range;
use std::sync::Arc;

/// Canaux de matériau d'un sommet : 4 et non [`VOXEL_MATERIALS`] (3), car Vulkan ne
/// garantit les formats de sommet 8/16 bits qu'en 4 composantes. Le 4ᵉ porte le rôle de
/// couture ; son poids reste à zéro.
const VERTEX_MATERIAL_CHANNELS: usize = 4;

// Rôle d'un sommet dans la couture. ⚠ Synchronisés avec `lib/chunk_seam.slang`.
/// Surface MC : comprimée près d'une face à dalle.
const SEAM_VERTEX_BODY: u16 = 0;
/// Jamais déplacé : dalle côté voisin, fond et parois.
pub const SEAM_VERTEX_FIXED: u16 = 1;
/// Dalle côté chunk : `+ i` pour la face d'index `i` dans `Face::ALL`.
pub const SEAM_VERTEX_SLAB_HIGH: u16 = 2;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct TerrainVertex {
    /// Coordonnées monde, avant la déformation de la couture.
    pub pos: Vec3,
    pub normal: Vec3, // normale lissée (gradient de densité), pour l'éclairage
    /// `R16G16B16A16_UINT` : 3 IDs de matériau (indices dans la table GPU), puis le rôle
    /// de couture.
    pub material_ids_and_seam_role: [u16; VERTEX_MATERIAL_CHANNELS],
    /// Poids associés (`R8G8B8A8_UNORM` : 255 → 1.0), somme 255.
    pub weights: [u8; VERTEX_MATERIAL_CHANNELS],
}

impl TerrainVertex {
    /// Sommet de rôle [`SEAM_VERTEX_BODY`]. Autre rôle : [`TerrainVertex::with_seam_role`].
    pub fn new(pos: Vec3, normal: Vec3, voxel: Voxel) -> TerrainVertex {
        let mut material_ids_and_seam_role = [0; VERTEX_MATERIAL_CHANNELS];
        let mut weights = [0; VERTEX_MATERIAL_CHANNELS];
        material_ids_and_seam_role[..VOXEL_MATERIALS].copy_from_slice(&voxel.materials);
        material_ids_and_seam_role[VOXEL_MATERIALS] = SEAM_VERTEX_BODY;
        weights[..VOXEL_MATERIALS].copy_from_slice(&voxel.weights);
        TerrainVertex {
            pos,
            normal,
            material_ids_and_seam_role,
            weights,
        }
    }

    pub fn with_seam_role(mut self, role: u16) -> TerrainVertex {
        self.material_ids_and_seam_role[VOXEL_MATERIALS] = role;
        self
    }

    /// Remplace les matériaux par `material` seul (teintes de debug), sans toucher au rôle.
    pub fn set_material(&mut self, material: u16) {
        let role = self.material_ids_and_seam_role[VOXEL_MATERIALS];
        *self = TerrainVertex::new(self.pos, self.normal, Voxel::solid(material))
            .with_seam_role(role);
    }

    pub fn get_binding_description() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<TerrainVertex>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    pub fn get_attribute_descriptions() -> [vk::VertexInputAttributeDescription; 4] {
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
                format: vk::Format::R16G16B16A16_UINT,
                offset: offset_of!(TerrainVertex, material_ids_and_seam_role) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 3,
                format: vk::Format::R8G8B8A8_UNORM,
                offset: offset_of!(TerrainVertex, weights) as u32,
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
    /// Le corps du chunk (surface, fond, parois), puis les dalles, à la suite.
    pub indices: Vec<u32>,
    /// Plage d'`indices` de la dalle de chaque face, dans l'ordre de `Face::ALL`.
    pub slabs: [Range<u32>; 4],
}

impl MeshData {
    /// Nombre d'index du corps : tout ce qui précède la première dalle.
    pub fn body_index_count(&self) -> u32 {
        self.slabs[0].start
    }

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

    /// Nombre d'index du corps du chunk, placés en tête de l'index buffer.
    #[inline]
    pub fn body_index_count(&self) -> u32 {
        self.data.body_index_count()
    }

    /// Plage d'index de la dalle de la face d'index `face` dans `Face::ALL`.
    #[inline]
    pub fn slab_indices(&self, face: usize) -> Range<u32> {
        self.data.slabs[face].clone()
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
