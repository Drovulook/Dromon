use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use std::mem::offset_of;
use std::sync::Arc;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ObjectVertex {
    pub pos: Vec3,
    pub color: Vec3,
    pub texCoord: Vec2,
    pub normal: Vec3,
}

impl ObjectVertex {
    pub fn get_binding_description() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<ObjectVertex>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    pub fn get_attribute_descriptions() -> [vk::VertexInputAttributeDescription; 4] {
        [
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 0,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: offset_of!(ObjectVertex, pos) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 1,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: offset_of!(ObjectVertex, color) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 2,
                format: vk::Format::R32G32_SFLOAT,
                offset: offset_of!(ObjectVertex, texCoord) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 3,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: offset_of!(ObjectVertex, normal) as u32,
            },
        ]
    }
}

pub struct Mesh {
    context: Arc<RenderingContext>,

    pub vertices: Vec<ObjectVertex>,
    pub vertex_staging_buffer: Buffer, // HACK: à terme, il faudra que ce buffer soit temporairement
    // détenu par le handler, puis supprimé à la fin de initialize
    pub vertex_buffer: Buffer,

    pub indices: Vec<u32>,
    pub index_staging_buffer: Buffer,
    pub index_buffer: Buffer,
}

impl Mesh {
    pub fn new(
        context: Arc<RenderingContext>,
        vertices: Vec<ObjectVertex>,
        vertex_staging_buffer: Buffer,
        vertex_buffer: Buffer,
        indices: Vec<u32>,
        index_staging_buffer: Buffer,
        index_buffer: Buffer,
    ) -> Mesh {
        Mesh {
            context,
            vertices,
            vertex_staging_buffer,
            vertex_buffer,
            indices,
            index_staging_buffer,
            index_buffer,
        }
    }

    pub fn bind(&self, command_buffer: vk::CommandBuffer) {
        unsafe {
            self.context.device.cmd_bind_vertex_buffers2(
                command_buffer,               // le command buffer en cours d'enregistrement
                0,                            // first_binding : index du premier binding (slot 0)
                &[self.vertex_buffer.buffer], // buffers : liste des buffers à binder
                &[0], // offsets : offset en bytes dans chaque buffer (0 = depuis le début)
                None, // sizes : taille à lire dans chaque buffer (None = jusqu'à la fin)
                None, // strides : override du stride défini dans le pipeline (None = utilise celui du pipeline)
            );
            self.context.device.cmd_bind_index_buffer(
                // HACK: utiliser ...buffers2 ?
                command_buffer,
                self.index_buffer.buffer,
                0,
                vk::IndexType::UINT32,
            );
        }
    }
}

pub const SQUARE_VERTICES: &[ObjectVertex] = &[
    ObjectVertex {
        pos: Vec3::new(-0.5, -0.5, 0.0),
        color: Vec3::new(1.0, 0.0, 0.0),
        texCoord: Vec2::new(1.0, 0.0),
        normal: Vec3::Z,
    },
    ObjectVertex {
        pos: Vec3::new(0.5, -0.5, 0.0),
        color: Vec3::new(0.0, 1.0, 0.0),
        texCoord: Vec2::new(0.0, 0.0),
        normal: Vec3::Z,
    },
    ObjectVertex {
        pos: Vec3::new(0.5, 0.5, 0.0),
        color: Vec3::new(0.0, 0.0, 1.0),
        texCoord: Vec2::new(0.0, 1.0),
        normal: Vec3::Z,
    },
    ObjectVertex {
        pos: Vec3::new(-0.5, 0.5, 0.0),
        color: Vec3::new(1.0, 1.0, 1.0),
        texCoord: Vec2::new(1.0, 1.0),
        normal: Vec3::Z,
    },
    // second square
    ObjectVertex {
        pos: Vec3::new(-0.5, -0.5, -0.5),
        color: Vec3::new(1.0, 0.0, 0.0),
        texCoord: Vec2::new(1.0, 0.0),
        normal: Vec3::NEG_Z,
    },
    ObjectVertex {
        pos: Vec3::new(0.5, -0.5, -0.5),
        color: Vec3::new(0.0, 1.0, 0.0),
        texCoord: Vec2::new(0.0, 0.0),
        normal: Vec3::NEG_Z,
    },
    ObjectVertex {
        pos: Vec3::new(0.5, 0.5, -0.5),
        color: Vec3::new(0.0, 0.0, 1.0),
        texCoord: Vec2::new(0.0, 1.0),
        normal: Vec3::NEG_Z,
    },
    ObjectVertex {
        pos: Vec3::new(-0.5, 0.5, -0.5),
        color: Vec3::new(1.0, 1.0, 1.0),
        texCoord: Vec2::new(1.0, 1.0),
        normal: Vec3::NEG_Z,
    },
];

pub const SQUARE_INDICES: &[u32] = &[0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4];
