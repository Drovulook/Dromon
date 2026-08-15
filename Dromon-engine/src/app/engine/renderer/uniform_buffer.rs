use super::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;
use crate::profile;
use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use std::sync::Arc;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct UniformBufferObject {
    view: Mat4,
    proj: Mat4,
    // Matrice « view*proj » de la lumière (projection orthographique depuis le
    // soleil).
    light_view_proj: Mat4,
    // des uniform buffers Vulkan : un vec3 est aligné sur 16 octets mais n'en
    // occupe que 12
    light_direction: Vec4,
    light_color: Vec4,
}

pub struct UniformBuffer {
    context: Arc<RenderingContext>,
    buffer: Buffer,
    mapped: *mut u8,
}

impl UniformBuffer {
    pub fn new(context: Arc<RenderingContext>) -> Result<Self> {
        let size = std::mem::size_of::<UniformBufferObject>() as vk::DeviceSize;
        let buffer = Buffer::new(
            context.clone(),
            size,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        let mapped = buffer.map()?;
        Ok(Self {
            context,
            buffer,
            mapped,
        })
    }

    pub fn get_handle(&self) -> vk::Buffer {
        self.buffer.buffer
    }

    pub fn update(
        &self,
        view: Mat4,
        proj: Mat4,
        light_view_proj: Mat4,
        light_direction: Vec3,
        light_color: Vec3,
        light_intensity: f32,
    ) {
        profile!();
        let ubo = UniformBufferObject {
            view,
            proj,
            light_view_proj,
            light_direction: light_direction.extend(0.0),
            light_color: light_color.extend(light_intensity),
        };

        unsafe {
            std::ptr::copy_nonoverlapping(
                &ubo as *const UniformBufferObject as *const u8,
                self.mapped,
                std::mem::size_of::<UniformBufferObject>(),
            );
        }
    }
}
