use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

pub struct DescriptorHandler {
    context: Arc<RenderingContext>,
    pub descriptor_pool: vk::DescriptorPool,
    pub world_descriptor_set_layout: vk::DescriptorSetLayout,
    pub texture_descriptor_set_layout: vk::DescriptorSetLayout,
    pub world_descriptor_sets: Vec<vk::DescriptorSet>,
}

impl DescriptorHandler {
    pub fn new(
        context: Arc<RenderingContext>,
        uniform_buffer_handles: Vec<vk::Buffer>,
    ) -> Result<Self> {
        let ubo_count = uniform_buffer_handles.len() as u32;
        let descriptor_pool = DescriptorHandler::create_descriptor_pool(&context, ubo_count)?;
        let (world_descriptor_set_layout, texture_descriptor_set_layout) =
            DescriptorHandler::create_descriptor_set_layouts(&context)?;
        let world_descriptor_sets = DescriptorHandler::create_world_descriptor_sets(
            &context,
            descriptor_pool,
            world_descriptor_set_layout,
            uniform_buffer_handles,
        )?;

        Ok(Self {
            context,
            descriptor_pool,
            world_descriptor_set_layout,
            texture_descriptor_set_layout,
            world_descriptor_sets,
        })
    }

    pub fn create_descriptor_set_layouts(
        context: &RenderingContext,
    ) -> Result<(vk::DescriptorSetLayout, vk::DescriptorSetLayout)> {
        let world_descriptor_set_layout = unsafe {
            context.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(0)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::VERTEX),
                ]),
                None,
            )?
        };

        let texture_descriptor_set_layout = unsafe {
            context.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(0) // ← binding 0 DANS le set 1
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                ]),
                None,
            )?
        };

        Ok((world_descriptor_set_layout, texture_descriptor_set_layout))
    }

    fn create_descriptor_pool(
        context: &RenderingContext,
        frame_count: u32,
    ) -> Result<vk::DescriptorPool> {
        const MAX_TEXTURES: u32 = 64;
        let pool_sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(frame_count),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(MAX_TEXTURES),
        ];

        unsafe {
            Ok(context.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    // .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                    .pool_sizes(&pool_sizes)
                    // frame_count sets UBO (un par frame) + un set par texture
                    .max_sets(frame_count + MAX_TEXTURES),
                None,
            )?)
        }
    }

    fn create_world_descriptor_sets(
        context: &RenderingContext,
        descriptor_pool: vk::DescriptorPool,
        descriptor_set_layout: vk::DescriptorSetLayout,
        uniform_buffer_handles: Vec<vk::Buffer>,
    ) -> Result<Vec<vk::DescriptorSet>> {
        let layouts = vec![descriptor_set_layout; uniform_buffer_handles.len()];

        let descriptor_sets = unsafe {
            context.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(descriptor_pool)
                    .set_layouts(&layouts),
            )?
        };

        let buffer_infos: Vec<[vk::DescriptorBufferInfo; 1]> = uniform_buffer_handles
            .iter()
            .map(|&handle| {
                [vk::DescriptorBufferInfo::default()
                    .buffer(handle)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)]
            })
            .collect();

        let writes: Vec<vk::WriteDescriptorSet> = descriptor_sets
            .iter()
            .zip(buffer_infos.iter())
            .map(|(&descriptor_set, buffer_info)| {
                // set 0, binding 0 : uniform buffer (caméra)
                vk::WriteDescriptorSet::default()
                    .dst_set(descriptor_set)
                    .dst_binding(0)
                    .dst_array_element(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(buffer_info)
            })
            .collect();

        unsafe {
            context.device.update_descriptor_sets(&writes, &[]);
        }

        Ok(descriptor_sets)
    }

    // called in texture_handler
    pub fn create_texture_descriptor_set(
        &self,
        texture_image_view: &vk::ImageView,
        texture_sampler: &vk::Sampler,
    ) -> Result<vk::DescriptorSet> {
        let descriptor_set = unsafe {
            self.context.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.descriptor_pool)
                    .set_layouts(&[self.texture_descriptor_set_layout]),
            )?
        }[0];
        let image_info = [vk::DescriptorImageInfo::default()
            .sampler(*texture_sampler)
            .image_view(*texture_image_view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info);

        unsafe { self.context.device.update_descriptor_sets(&[write], &[]) };

        Ok(descriptor_set)
    }
}

impl Drop for DescriptorHandler {
    fn drop(&mut self) {
        unsafe {
            self.context
                .device
                .destroy_descriptor_pool(self.descriptor_pool, None);
            self.context
                .device
                .destroy_descriptor_set_layout(self.world_descriptor_set_layout, None);
            self.context
                .device
                .destroy_descriptor_set_layout(self.texture_descriptor_set_layout, None);
        }
    }
}
