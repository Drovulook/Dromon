use std::sync::Arc;

use anyhow::Result;
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
    pub vertex_buffer: vk::Buffer,
    pub vertex_buffer_memory: vk::DeviceMemory,
    context: Arc<RenderingContext>,
}

impl Model {
    pub fn new(context: Arc<RenderingContext>, vertices: Vec<Vertex>) -> Result<Self> {
        let (vertex_buffer, vertex_buffer_memory) =
            Model::create_vertex_buffer(&context, &vertices)?;

        unsafe {
            let data = context.device.map_memory(
                vertex_buffer_memory,
                0,
                std::mem::size_of_val(vertices.as_slice()) as u64,
                vk::MemoryMapFlags::empty(),
            )? as *mut Vertex;
            std::ptr::copy_nonoverlapping(vertices.as_ptr(), data, vertices.len());
            context.device.unmap_memory(vertex_buffer_memory);
        }

        Ok(Self {
            vertices,
            vertex_buffer,
            vertex_buffer_memory,
            context,
        })
    }

    fn create_vertex_buffer(
        context: &RenderingContext,
        vertices: &[Vertex],
    ) -> Result<(vk::Buffer, vk::DeviceMemory)> {
        let vertex_buffer = unsafe {
            context.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(std::mem::size_of_val(vertices) as u64)
                    .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?
        };

        let memory_requirements =
            unsafe { context.device.get_buffer_memory_requirements(vertex_buffer) };

        let vertex_buffer_memory = unsafe {
            context.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(memory_requirements.size)
                    .memory_type_index(Model::find_memory_type(
                        context,
                        memory_requirements.memory_type_bits,
                        vk::MemoryPropertyFlags::HOST_VISIBLE,
                    )?),
                None,
            )?
        };
        unsafe {
            context
                .device
                .bind_buffer_memory(vertex_buffer, vertex_buffer_memory, 0)?;
        }
        Ok((vertex_buffer, vertex_buffer_memory))
    }

    fn find_memory_type(
        context: &RenderingContext,
        type_filter: u32,
        properties: vk::MemoryPropertyFlags,
    ) -> Result<u32> {
        let memory_properties = unsafe {
            context
                .instance
                .get_physical_device_memory_properties(context.physical_device.handle)
        };
        (0..memory_properties.memory_type_count)
            .find(|&i| {
                (type_filter & (1 << i) != 0)
                    && (memory_properties.memory_types[i as usize].property_flags & properties)
                        == properties
            })
            .ok_or_else(|| anyhow::anyhow!("Aucun type mémoire compatible trouvé"))
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        unsafe {
            let _ = self.context.device.device_wait_idle();
            self.context.device.destroy_buffer(self.vertex_buffer, None);
            self.context
                .device
                .free_memory(self.vertex_buffer_memory, None);
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
