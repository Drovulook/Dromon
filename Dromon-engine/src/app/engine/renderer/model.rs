use std::sync::Arc;

use ash::vk;
use glam::{Vec2, Vec3};

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
    pub vertices: Vec<Vertex>,
    context: Arc<RenderingContext>,
    // pub vertex_buffer: vk::Buffer,
    // pub vertex_buffer_memory: vk::DeviceMemory,
}

impl Model {
    pub fn new(context: Arc<RenderingContext>, vertices: Vec<Vertex>) -> Self {
        Model::create_vertex_buffer(&context, &vertices);
        Self { vertices, context }
    }

    fn create_vertex_buffer(context: &RenderingContext, vertices: &[Vertex]) {
        let vertex_buffer = unsafe {
            context
                .device
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(std::mem::size_of_val(vertices) as u64)
                        .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE),
                    None,
                )
                .unwrap()
        };
    }
}

const VERTICES: &[Vertex] = &[
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
