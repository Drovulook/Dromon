use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

pub struct DescriptorHandler {
    context: Arc<RenderingContext>,
    pub descriptor_pool: vk::DescriptorPool,
    pub world_descriptor_set_layout: vk::DescriptorSetLayout,
    pub texture_descriptor_set_layout: vk::DescriptorSetLayout,
    pub shadow_descriptor_set_layout: vk::DescriptorSetLayout,
    pub world_descriptor_sets: Vec<vk::DescriptorSet>,
    // Set 2 : la shadow map. Une seule ressource (image unique) partagée par
    // toutes les frames, donc un seul descriptor set, rebindé chaque frame.
    pub shadow_descriptor_set: vk::DescriptorSet,
}

impl DescriptorHandler {
    pub fn new(
        context: Arc<RenderingContext>,
        uniform_buffer_handles: Vec<vk::Buffer>,
        shadow_map_view: vk::ImageView,
        shadow_map_sampler: vk::Sampler,
    ) -> Result<Self> {
        let ubo_count = uniform_buffer_handles.len() as u32;
        let descriptor_pool = DescriptorHandler::create_descriptor_pool(&context, ubo_count)?;
        let (
            world_descriptor_set_layout,
            texture_descriptor_set_layout,
            shadow_descriptor_set_layout,
        ) = DescriptorHandler::create_descriptor_set_layouts(&context)?;
        let world_descriptor_sets = DescriptorHandler::create_world_descriptor_sets(
            &context,
            descriptor_pool,
            world_descriptor_set_layout,
            uniform_buffer_handles,
        )?;
        let shadow_descriptor_set = DescriptorHandler::create_shadow_descriptor_set(
            &context,
            descriptor_pool,
            shadow_descriptor_set_layout,
            shadow_map_view,
            shadow_map_sampler,
        )?;

        Ok(Self {
            context,
            descriptor_pool,
            world_descriptor_set_layout,
            texture_descriptor_set_layout,
            shadow_descriptor_set_layout,
            world_descriptor_sets,
            shadow_descriptor_set,
        })
    }

    pub fn create_descriptor_set_layouts(
        context: &RenderingContext,
    ) -> Result<(
        vk::DescriptorSetLayout,
        vk::DescriptorSetLayout,
        vk::DescriptorSetLayout,
    )> {
        let world_descriptor_set_layout = unsafe {
            context.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(0)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .descriptor_count(1)
                        // VERTEX : matrices view/proj. FRAGMENT : données de lumière.
                        .stage_flags(
                            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                        ),
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

        // Set 2 : la shadow map, échantillonnée uniquement dans le frag shader.
        let shadow_descriptor_set_layout = unsafe {
            context.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                ]),
                None,
            )?
        };

        Ok((
            world_descriptor_set_layout,
            texture_descriptor_set_layout,
            shadow_descriptor_set_layout,
        ))
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
            // +1 : la shadow map est aussi un combined image sampler.
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(MAX_TEXTURES + 1),
        ];

        unsafe {
            Ok(context.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    // .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                    .pool_sizes(&pool_sizes)
                    // frame_count sets UBO + un set par texture + 1 set shadow map
                    .max_sets(frame_count + MAX_TEXTURES + 1),
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

    /// Alloue et écrit l'unique descriptor set de la shadow map (set 2). L'image
    /// est déclarée en SHADER_READ_ONLY_OPTIMAL : c'est l'état dans lequel on la
    /// transitionne après la passe d'ombre, juste avant de l'échantillonner.
    fn create_shadow_descriptor_set(
        context: &RenderingContext,
        descriptor_pool: vk::DescriptorPool,
        shadow_descriptor_set_layout: vk::DescriptorSetLayout,
        shadow_map_view: vk::ImageView,
        shadow_map_sampler: vk::Sampler,
    ) -> Result<vk::DescriptorSet> {
        let descriptor_set = unsafe {
            context.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(descriptor_pool)
                    .set_layouts(&[shadow_descriptor_set_layout]),
            )?
        }[0];

        let image_info = [vk::DescriptorImageInfo::default()
            .sampler(shadow_map_sampler)
            .image_view(shadow_map_view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info);

        unsafe { context.device.update_descriptor_sets(&[write], &[]) };

        Ok(descriptor_set)
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
            self.context
                .device
                .destroy_descriptor_set_layout(self.shadow_descriptor_set_layout, None);
        }
    }
}
