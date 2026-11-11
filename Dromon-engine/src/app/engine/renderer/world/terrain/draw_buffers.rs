//! **Draw indirect du terrain** : les commandes de dessin des chunks visibles, écrites par
//! le CPU dans des buffers que le GPU lit lui-même. Un seul `cmd_draw_indexed_indirect`
//! par passe, au lieu d'un draw (et d'une push constant) par chunk.
//!
//! La couture de chaque chunk est un attribut **par instance** : sa commande la désigne
//! par `first_instance` (cf. `ChunkSeam::binding_description`).

use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use super::culling::VisibleSet;
use super::meshes::{ChunkSeam, InstalledChunks};
use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;

/// Buffer host-visible recréé plus grand quand les données ne tiennent plus.
struct GrowableBuffer {
    buffer: Option<Buffer>,
    /// En éléments.
    capacity: usize,
    usage: vk::BufferUsageFlags,
}

impl GrowableBuffer {
    fn new(usage: vk::BufferUsageFlags) -> Self {
        Self {
            buffer: None,
            capacity: 0,
            usage,
        }
    }

    /// ⚠ Le GPU ne doit plus lire ce buffer : l'ancien est détruit s'il faut grandir.
    fn write<T: Copy>(&mut self, context: &Arc<RenderingContext>, data: &[T]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        if data.len() > self.capacity {
            // Marge de 50 % : le nombre de chunks visibles varie d'une frame à l'autre.
            let capacity = data.len() * 3 / 2;
            self.buffer = Some(Buffer::new(
                context.clone(),
                (capacity * size_of::<T>()) as u64,
                self.usage,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            )?);
            self.capacity = capacity;
        }
        match &self.buffer {
            Some(buffer) => buffer.upload(data),
            None => Ok(()),
        }
    }

    fn handle(&self) -> vk::Buffer {
        self.buffer
            .as_ref()
            .map_or(vk::Buffer::null(), |buffer| buffer.buffer)
    }
}

/// Les buffers d'une frame en vol, et ce qu'ils contiennent.
struct FrameDraws {
    camera: GrowableBuffer,
    shadow: GrowableBuffer,
    seams: GrowableBuffer,
    camera_count: u32,
    shadow_count: u32,
}

/// Commandes à exécuter par une passe : buffer et nombre de commandes.
pub(crate) struct IndirectDraws {
    pub buffer: vk::Buffer,
    pub count: u32,
}

impl IndirectDraws {
    pub const STRIDE: u32 = size_of::<vk::DrawIndexedIndirectCommand>() as u32;
}

pub struct DrawBuffers {
    context: Arc<RenderingContext>,
    /// Un jeu par frame en vol : le GPU lit celui de la frame `i` pendant que le CPU
    /// remplit celui de la suivante.
    frames: Vec<FrameDraws>,
    // Tampons CPU réutilisés d'une frame à l'autre, puis copiés d'un bloc.
    camera: Vec<vk::DrawIndexedIndirectCommand>,
    shadow: Vec<vk::DrawIndexedIndirectCommand>,
    seams: Vec<ChunkSeam>,
}

impl DrawBuffers {
    pub(super) fn new(context: Arc<RenderingContext>, frames_in_flight: usize) -> Self {
        let frames = (0..frames_in_flight)
            .map(|_| FrameDraws {
                camera: GrowableBuffer::new(vk::BufferUsageFlags::INDIRECT_BUFFER),
                shadow: GrowableBuffer::new(vk::BufferUsageFlags::INDIRECT_BUFFER),
                seams: GrowableBuffer::new(vk::BufferUsageFlags::VERTEX_BUFFER),
                camera_count: 0,
                shadow_count: 0,
            })
            .collect();
        Self {
            context,
            frames,
            camera: Vec::new(),
            shadow: Vec::new(),
            seams: Vec::new(),
        }
    }

    /// Écrit les commandes des chunks visibles dans les buffers de `frame`, dans l'ordre
    /// de `visible` (du proche au lointain pour la caméra).
    ///
    /// ⚠ À appeler **après** l'attente du fence de `frame` : avant, le GPU peut encore
    /// lire ces buffers.
    pub(super) fn write(
        &mut self,
        frame: usize,
        chunks: &InstalledChunks,
        visible: &VisibleSet,
    ) -> Result<()> {
        self.camera.clear();
        self.shadow.clear();
        self.seams.clear();

        for coord in &visible.camera {
            let Some(chunk) = chunks.get(coord) else {
                continue;
            };
            let instance = self.seams.len() as u32;
            self.seams.push(chunk.seam);
            let body = std::iter::once(chunk.mesh.body_draw());
            let slabs = (0..4)
                .filter(|&face| chunk.seam.has_slab(face))
                .map(|face| chunk.mesh.slab_draw(face));
            for mut draw in body.chain(slabs) {
                if draw.index_count > 0 {
                    draw.first_instance = instance;
                    self.camera.push(draw);
                }
            }
        }
        // Corps seul, sans couture : le pipeline d'ombre n'a pas d'attribut par instance.
        for coord in &visible.shadow {
            if let Some(chunk) = chunks.get(coord) {
                let draw = chunk.mesh.body_draw();
                if draw.index_count > 0 {
                    self.shadow.push(draw);
                }
            }
        }

        let buffers = &mut self.frames[frame];
        buffers.camera.write(&self.context, &self.camera)?;
        buffers.shadow.write(&self.context, &self.shadow)?;
        buffers.seams.write(&self.context, &self.seams)?;
        buffers.camera_count = self.camera.len() as u32;
        buffers.shadow_count = self.shadow.len() as u32;
        Ok(())
    }

    /// Commandes de la passe principale de `frame`.
    pub(crate) fn camera(&self, frame: usize) -> IndirectDraws {
        let buffers = &self.frames[frame];
        IndirectDraws {
            buffer: buffers.camera.handle(),
            count: buffers.camera_count,
        }
    }

    /// Commandes de la passe d'ombre de `frame`.
    pub(crate) fn shadow(&self, frame: usize) -> IndirectDraws {
        let buffers = &self.frames[frame];
        IndirectDraws {
            buffer: buffers.shadow.handle(),
            count: buffers.shadow_count,
        }
    }

    /// Coutures de `frame`, à binder sur `ChunkSeam::BINDING`.
    pub(crate) fn seams(&self, frame: usize) -> vk::Buffer {
        self.frames[frame].seams.handle()
    }
}
