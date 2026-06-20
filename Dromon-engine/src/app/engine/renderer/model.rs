use super::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use std::mem::offset_of;
use std::sync::Arc;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pos: Vec2,
    color: Vec3,
    texCoord: Vec2,
}

impl Vertex {
    pub fn get_binding_description() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Vertex>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    pub fn get_attribute_descriptions() -> [vk::VertexInputAttributeDescription; 3] {
        [
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: offset_of!(Vertex, pos) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 1,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: offset_of!(Vertex, color) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 2,
                format: vk::Format::R32G32_SFLOAT,
                offset: offset_of!(Vertex, texCoord) as u32,
            },
        ]
    }
}

pub struct Model {
    context: Arc<RenderingContext>,

    pub vertices: Vec<Vertex>,
    vertex_staging_buffer: Buffer,
    pub vertex_buffer: Buffer,

    pub indices: Vec<u32>,
    index_staging_buffer: Buffer,
    pub index_buffer: Buffer,
}

impl Model {
    pub fn new(
        context: Arc<RenderingContext>,
        vertices: Vec<Vertex>,
        indices: Vec<u32>,
    ) -> Result<Self> {
        let (vertex_staging_buffer, vertex_buffer) = Self::create_staging_and_device_buffer(
            context.clone(),
            vertices.as_slice(),
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;

        let (index_staging_buffer, index_buffer) = Self::create_staging_and_device_buffer(
            context.clone(),
            indices.as_slice(),
            vk::BufferUsageFlags::INDEX_BUFFER,
        )?;

        Ok(Self {
            context,
            vertices,
            vertex_staging_buffer,
            vertex_buffer,
            indices,
            index_staging_buffer,
            index_buffer,
        })
    }

    pub fn create_staging_and_device_buffer<T: Copy>(
        context: Arc<RenderingContext>,
        data: &[T],
        role: vk::BufferUsageFlags,
    ) -> Result<(Buffer, Buffer)> {
        let staging_buffer = Buffer::new(
            context.clone(),
            std::mem::size_of_val(data) as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        staging_buffer.map_and_unmap(data)?;

        let device_buffer = Buffer::new(
            context.clone(),
            std::mem::size_of_val(data) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | role,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        Ok((staging_buffer, device_buffer))
    }

    pub fn copy_from_staging_to_device(&self, command_buffer: &vk::CommandBuffer) {
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

    pub fn bind_vertex_buffer(&self, command_buffer: vk::CommandBuffer) {
        unsafe {
            self.context.device.cmd_bind_vertex_buffers2(
                command_buffer,               // le command buffer en cours d'enregistrement
                0,                            // first_binding : index du premier binding (slot 0)
                &[self.vertex_buffer.buffer], // buffers : liste des buffers à binder
                &[0], // offsets : offset en bytes dans chaque buffer (0 = depuis le début)
                None, // sizes : taille à lire dans chaque buffer (None = jusqu'à la fin)
                None, // strides : override du stride défini dans le pipeline (None = utilise celui du pipeline)
            );
        }
    }

    pub fn bind_index_buffer(&self, command_buffer: vk::CommandBuffer) {
        unsafe {
            self.context.device.cmd_bind_index_buffer(
                command_buffer,
                self.index_buffer.buffer,
                0,
                vk::IndexType::UINT32,
            );
        }
    }
}

pub const TRIANGLE_VERTICES: &[Vertex] = &[
    Vertex {
        pos: Vec2::new(-0.5, -0.5),
        color: Vec3::new(1.0, 0.0, 0.0),
        texCoord: Vec2::new(1.0, 0.0),
    },
    Vertex {
        pos: Vec2::new(0.5, -0.5),
        color: Vec3::new(0.0, 1.0, 0.0),
        texCoord: Vec2::new(0.0, 0.0),
    },
    Vertex {
        pos: Vec2::new(0.5, 0.5),
        color: Vec3::new(0.0, 0.0, 1.0),
        texCoord: Vec2::new(0.0, 1.0),
    },
    Vertex {
        pos: Vec2::new(-0.5, 0.5),
        color: Vec3::new(1.0, 1.0, 1.0),
        texCoord: Vec2::new(1.0, 1.0),
    },
];

pub const TRIANGLE_INDICES: &[u32] = &[0, 1, 2, 2, 3, 0];
