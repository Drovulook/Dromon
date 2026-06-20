use super::buffer::Buffer;
use crate::app::engine::Timer;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::ffi::c_void;
use std::sync::Arc;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct UniformBufferObject {
    model: Mat4,
    view: Mat4,
    proj: Mat4,
}

pub struct UniformBuffer {
    context: Arc<RenderingContext>,
    buffer: Buffer,
    mapped: *mut c_void,
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
        let mapped = buffer.map(size)?;
        Ok(Self {
            context,
            buffer,
            mapped,
        })
    }

    pub fn get_handle(&self) -> vk::Buffer {
        self.buffer.buffer
    }

    pub fn update(&self, time: &Timer, swapchain_width: f32, swapchain_height: f32) {
        let mut ubo = UniformBufferObject {
            model: Mat4::from_rotation_z(time.elapsed_secs() * 90.0_f32.to_radians() * 0.6),
            view: Mat4::look_at_rh(
                Vec3::new(2.0, 2.0, 2.0), // eye (position de la caméra)
                Vec3::new(0.0, 0.0, 0.0), // center (point regardé)
                Vec3::new(0.0, 0.0, 1.0), // up (vecteur « haut »)
            ),
            proj: Mat4::perspective_rh(
                45.0_f32.to_radians(),                            // fov vertical (en radians)
                swapchain_width as f32 / swapchain_height as f32, // aspect ratio
                0.1,                                              // near plane
                10.0,                                             // far plane
            ),
        };
        ubo.proj.y_axis.y *= -1.0;

        unsafe {
            std::ptr::copy_nonoverlapping(
                &ubo as *const UniformBufferObject,
                self.mapped as *mut UniformBufferObject,
                1,
            );
        }
    }
}
