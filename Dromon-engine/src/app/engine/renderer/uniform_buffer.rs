use super::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use std::sync::Arc;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct UniformBufferObject {
    model: Mat4,
    view: Mat4,
    proj: Mat4,
}

struct UniformBuffer {
    context: Arc<RenderingContext>,
    buffer: Buffer,
}

impl UniformBuffer {
    pub fn new(context: Arc<RenderingContext>) {}

    fn create_descriptor_set_layout(context: Arc<RenderingContext>) {
        let ubo_layout_binding = vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::VERTEX);
    }
}
