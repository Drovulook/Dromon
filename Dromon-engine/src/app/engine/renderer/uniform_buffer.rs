use super::buffer::Buffer;
use crate::app::engine::Timer;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use std::ffi::c_void;
use std::sync::Arc;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct UniformBufferObject {
    model: Mat4,
    view: Mat4,
    proj: Mat4,
}

pub fn create_descriptor_set_layout(context: &RenderingContext) -> Result<vk::DescriptorSetLayout> {
    let ubo_layout_binding = vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::VERTEX);

    let layout = unsafe {
        context.device.create_descriptor_set_layout(
            &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[ubo_layout_binding]),
            None,
        )
    }?;

    Ok(layout)
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

    pub fn update(&self, time: &Timer) {
        let ubo = UniformBufferObject {
            model: Mat4::from_rotation_z(time.elapsed_secs() * 90.0_f32.to_radians()),
            view: Mat4::IDENTITY,
            proj: Mat4::IDENTITY,
        };
        unsafe {
            std::ptr::copy_nonoverlapping(
                &ubo as *const UniformBufferObject,
                self.mapped as *mut UniformBufferObject,
                1,
            );
        }
    }
}
