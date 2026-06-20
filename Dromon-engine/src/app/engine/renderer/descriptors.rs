use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

pub struct DescriptorHandler {
    context: Arc<RenderingContext>,
    descriptor_pool: vk::DescriptorPool,
    pub ubo_descriptor_set_layout: vk::DescriptorSetLayout,
    pub ubo_descriptor_sets: Vec<vk::DescriptorSet>,
}

impl DescriptorHandler {
    pub fn new(
        context: Arc<RenderingContext>,
        uniform_buffer_handles: Vec<vk::Buffer>,
        texture_image_view: &vk::ImageView,
        texture_sampler: &vk::Sampler,
    ) -> Result<Self> {
        let ubo_count = uniform_buffer_handles.len() as u32;
        let descriptor_pool = DescriptorHandler::create_descriptor_pool(&context, ubo_count)?;
        let ubo_descriptor_set_layout =
            DescriptorHandler::create_ubo_descriptor_set_layout(&context)?;
        let ubo_descriptor_sets = DescriptorHandler::create_ubo_and_image_descriptor_sets(
            &context,
            descriptor_pool,
            ubo_descriptor_set_layout,
            uniform_buffer_handles,
            texture_image_view,
            texture_sampler,
        )?;

        Ok(Self {
            context,
            descriptor_pool,
            ubo_descriptor_set_layout,
            ubo_descriptor_sets,
        })
    }

    fn create_ubo_descriptor_set_layout(
        context: &RenderingContext,
    ) -> Result<vk::DescriptorSetLayout> {
        let ubo_layout_binding = vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::VERTEX);
        let combined_image_sampler_layout_binding = vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT);

        let layout = unsafe {
            context.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default()
                    .bindings(&[ubo_layout_binding, combined_image_sampler_layout_binding]),
                None,
            )
        }?;

        Ok(layout)
    }

    fn create_descriptor_pool(
        context: &RenderingContext,
        frame_count: u32,
    ) -> Result<vk::DescriptorPool> {
        let pool_sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(frame_count),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(frame_count),
        ];

        unsafe {
            Ok(context.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    // .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                    .pool_sizes(&pool_sizes)
                    .max_sets(frame_count),
                None,
            )?)
        }
    }

    fn create_ubo_and_image_descriptor_sets(
        context: &RenderingContext,
        descriptor_pool: vk::DescriptorPool,
        descriptor_set_layout: vk::DescriptorSetLayout,
        uniform_buffer_handles: Vec<vk::Buffer>,
        texture_image_view: &vk::ImageView,
        texture_sampler: &vk::Sampler,
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

        let image_infos: Vec<[vk::DescriptorImageInfo; 1]> =
            vec![
                [vk::DescriptorImageInfo::default()
                    .sampler(*texture_sampler)
                    .image_view(*texture_image_view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
                2
            ];

        let writes: Vec<vk::WriteDescriptorSet> = descriptor_sets
            .iter()
            .zip(buffer_infos.iter())
            .zip(image_infos.iter())
            .flat_map(|((&descriptor_set, buffer_info), image_info)| {
                [
                    // binding 0: uniform buffer
                    vk::WriteDescriptorSet::default()
                        .dst_set(descriptor_set)
                        .dst_binding(0)
                        .dst_array_element(0)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .buffer_info(buffer_info),
                    // binding 1: combined image sampler
                    vk::WriteDescriptorSet::default()
                        .dst_set(descriptor_set)
                        .dst_binding(1)
                        .dst_array_element(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(image_info),
                ]
            })
            .collect();

        unsafe {
            context.device.update_descriptor_sets(&writes, &[]);
        }

        Ok(descriptor_sets)
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
                .destroy_descriptor_set_layout(self.ubo_descriptor_set_layout, None)
        }
    }
}
