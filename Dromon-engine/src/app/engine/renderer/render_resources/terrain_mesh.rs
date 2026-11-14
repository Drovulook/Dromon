use crate::app::engine::renderer::gpu_memory::arena::ArenaSlice;
use crate::app::engine::renderer::gpu_memory::staging_ring::StagingRing;
use crate::app::engine::renderer::world::terrain::arenas::TerrainArenas;
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
    /// Poids associés (`R8G8B8A8_UINT`, entiers : le shader les compacte tels quels),
    /// somme 255.
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
                format: vk::Format::R8G8B8A8_UINT,
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

/// Faute de place, un mesh n'a pas pu être construit ; rien n'est réservé.
pub enum NoRoom {
    /// Une arène est pleine : à signaler, il faut l'agrandir.
    Arena,
    /// Budget d'envoi de la frame atteint : normal, à retenter à la frame suivante.
    Staging,
}

/// Les index suivent les sommets dans le ring, à un offset aligné (cf. `StagingRing`).
const STAGING_INDEX_ALIGN: u64 = 16;

/// Le mesh d'un chunk de terrain, côté GPU : deux tranches des arènes du terrain. Ses
/// données transitent par le ring de staging le temps d'une copie.
///
/// Pas de `Drop` : les tranches se rendent par [`TerrainMesh::release`], qui a accès
/// aux arènes. Un mesh simplement droppé laisse ses tranches occupées.
pub struct TerrainMesh {
    context: Arc<RenderingContext>,
    /// Géométrie source, partagée avec le cache multi-LOD.
    pub data: Arc<MeshData>,
    vertices: ArenaSlice,
    indices: ArenaSlice,
    /// Position des sommets dans le ring, valable jusqu'à [`TerrainMesh::record_upload`]
    /// de la même frame.
    staging_offset: u64,
}

impl TerrainMesh {
    /// Réserve les tranches et copie les données dans le ring de staging. La copie vers
    /// les arènes s'enregistre **dans la même frame** via [`TerrainMesh::record_upload`] :
    /// la zone du ring est recyclée quelques frames plus tard.
    pub fn new(
        context: Arc<RenderingContext>,
        data: Arc<MeshData>,
        arenas: &mut TerrainArenas,
        ring: &mut StagingRing,
    ) -> Result<Result<TerrainMesh, NoRoom>> {
        profile!();
        // Arènes d'abord : leurs tranches se rendent aussitôt, pas une réservation du ring.
        let Some(vertices) = arenas.vertices.allocate(data.vertices.len() as u32) else {
            return Ok(Err(NoRoom::Arena));
        };
        let Some(indices) = arenas.indices.allocate(data.indices.len() as u32) else {
            arenas.vertices.free(vertices);
            return Ok(Err(NoRoom::Arena));
        };
        let index_start = Self::staging_index_start(&data);
        let index_bytes = std::mem::size_of_val(data.indices.as_slice()) as u64;
        let Some(staging_offset) = ring.allocate(index_start + index_bytes) else {
            arenas.vertices.free(vertices);
            arenas.indices.free(indices);
            return Ok(Err(NoRoom::Staging));
        };
        ring.write(staging_offset, &data.vertices)?;
        ring.write(staging_offset + index_start, &data.indices)?;

        Ok(Ok(TerrainMesh {
            context,
            data,
            vertices,
            indices,
            staging_offset,
        }))
    }

    /// Début des index dans la réservation du ring, après les sommets.
    fn staging_index_start(data: &MeshData) -> u64 {
        (std::mem::size_of_val(data.vertices.as_slice()) as u64)
            .next_multiple_of(STAGING_INDEX_ALIGN)
    }

    /// Rend les tranches aux arènes. ⚠ Aucune frame en vol ne doit plus les lire
    /// (cf. `Graveyard`).
    pub fn release(self, arenas: &mut TerrainArenas) {
        arenas.vertices.free(self.vertices);
        arenas.indices.free(self.indices);
    }

    /// Draw du corps du chunk, placé en tête de ses index.
    pub fn body_draw(&self) -> vk::DrawIndexedIndirectCommand {
        self.draw(0..self.data.body_index_count())
    }

    /// Draw de la dalle de la face d'index `face` dans `Face::ALL`.
    pub fn slab_draw(&self, face: usize) -> vk::DrawIndexedIndirectCommand {
        self.draw(self.data.slabs[face].clone())
    }

    /// `indices` : plage relative au chunk. `vertex_offset` est ajouté à chaque index
    /// lu : les index du mailleur, relatifs au chunk, restent valides dans l'arène.
    fn draw(&self, indices: Range<u32>) -> vk::DrawIndexedIndirectCommand {
        vk::DrawIndexedIndirectCommand {
            index_count: indices.end - indices.start,
            instance_count: 1,
            first_index: self.indices.first + indices.start,
            vertex_offset: self.vertices.first as i32,
            first_instance: 0,
        }
    }

    /// Enregistre la copie ring → arènes, **dans la frame qui a construit le mesh**.
    /// L'appelant pose derrière une barrière `TRANSFER_WRITE → VERTEX_ATTRIBUTE_READ |
    /// INDEX_READ` (cf. `Terrain::record_uploads`).
    pub fn record_upload(
        &self,
        command_buffer: &vk::CommandBuffer,
        arenas: &TerrainArenas,
        ring: &StagingRing,
    ) {
        let copies = [
            (
                self.staging_offset,
                &arenas.vertices,
                &self.vertices,
                std::mem::size_of_val(self.data.vertices.as_slice()),
            ),
            (
                self.staging_offset + Self::staging_index_start(&self.data),
                &arenas.indices,
                &self.indices,
                std::mem::size_of_val(self.data.indices.as_slice()),
            ),
        ];
        for (src_offset, arena, slice, size) in copies {
            unsafe {
                self.context.device.cmd_copy_buffer(
                    *command_buffer,
                    ring.buffer(),
                    arena.buffer(),
                    &[vk::BufferCopy {
                        src_offset,
                        dst_offset: arena.byte_offset(slice),
                        size: size as u64,
                    }],
                );
            }
        }
    }
}
