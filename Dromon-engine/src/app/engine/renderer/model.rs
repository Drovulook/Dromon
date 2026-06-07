use super::buffer::Buffer;
use anyhow::Result;
use ash::vk;
use glam::{Vec2, Vec3};
use std::sync::Arc;

use crate::app::engine::rendering_context::RenderingContext;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pos: Vec2,
    color: Vec3,
}

impl Vertex {
    pub fn get_binding_description() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Vertex>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    pub fn get_attribute_descriptions() -> [vk::VertexInputAttributeDescription; 2] {
        [
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 0,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 1,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: 8,
            },
        ]
    }
}

pub struct Model {
    context: Arc<RenderingContext>,
    pub vertices: Vec<Vertex>,
    staging_buffer: Buffer,
    pub vertex_buffer: Buffer,
}

impl Model {
    pub fn new(context: Arc<RenderingContext>, vertices: Vec<Vertex>) -> Result<Self> {
        let staging_buffer = Buffer::new(
            context.clone(),
            std::mem::size_of_val(vertices.as_slice()) as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        staging_buffer.map_and_unmap(vertices.as_slice())?;

        let vertex_buffer = Buffer::new(
            context.clone(),
            std::mem::size_of_val(vertices.as_slice()) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        Ok(Self {
            context,
            vertices,
            staging_buffer,
            vertex_buffer,
        })
    }

    pub fn copy_from_staging_to_device(&self, command_buffer: &vk::CommandBuffer) {
        unsafe {
            self.context.device.cmd_copy_buffer(
                *command_buffer,
                self.staging_buffer.buffer,
                self.vertex_buffer.buffer,
                &[vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: std::mem::size_of_val(self.vertices.as_slice()) as u64,
                }],
            );
        }
    }
}

pub const TRIANGLE_VERTICES: &[Vertex] = &[
    Vertex {
        pos: Vec2::new(0.0, -0.5),
        color: Vec3::new(1.0, 0.0, 0.0),
    },
    Vertex {
        pos: Vec2::new(0.5, 0.5),
        color: Vec3::new(0.0, 1.0, 0.0),
    },
    Vertex {
        pos: Vec2::new(-0.5, 0.5),
        color: Vec3::new(0.0, 0.0, 1.0),
    },
];
