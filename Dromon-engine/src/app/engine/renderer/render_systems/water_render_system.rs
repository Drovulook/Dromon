use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use crate::app::engine::{
    renderer::{
        descriptors::DescriptorHandler,
        pipelines,
        render_resources::{WATER_GRID_RESOLUTION, WaterVertex, WorldMaps},
        world::water::{Water, WaterPushConstants},
    },
    rendering_context::RenderingContext,
};

/// Rendu de la mer : la grille de `Water` redessinée pour chaque nœud.
/// Pas de passe d'ombre : l'eau n'en projette pas.
pub(crate) struct WaterRenderSystem {
    context: Arc<RenderingContext>,
    descriptor_handler: Arc<DescriptorHandler>,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
}

impl WaterRenderSystem {
    pub(crate) fn new(
        context: Arc<RenderingContext>,
        descriptor_handler: Arc<DescriptorHandler>,
        color_format: vk::Format,
        depth_format: vk::Format,
        msaa_samples: vk::SampleCountFlags,
    ) -> Result<Self> {
        unsafe {
            let vertex_shader = pipelines::load_shader_module(&context.device,"water_vert.spv")?;
            let fragment_shader = pipelines::load_shader_module(&context.device,"water_frag.spv")?;

            // Le fragment shader lit aussi les push constants (niveau, mode debug).
            let push_constant_ranges = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .offset(0)
                .size(std::mem::size_of::<WaterPushConstants>() as u32)];

            let pipeline_layout = context.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[
                        // set 0 : UBO caméra (par frame)
                        descriptor_handler.world_descriptor_set_layout,
                        // sets 1 et 2 inutilisés par l'eau, mais un layout doit
                        // déclarer tous les sets jusqu'au dernier.
                        descriptor_handler.texture_descriptor_set_layout,
                        descriptor_handler.shadow_descriptor_set_layout,
                        // set 3 : cartes du monde
                        descriptor_handler.world_maps_descriptor_set_layout,
                    ])
                    .push_constant_ranges(&push_constant_ranges),
                None,
            )?;

            let pipeline = pipelines::create_water_pipeline(
                &context.device,
                vertex_shader,
                fragment_shader,
                pipeline_layout,
                &[WaterVertex::get_binding_description()],
                &WaterVertex::get_attribute_descriptions(),
                color_format,
                depth_format,
                msaa_samples,
                vk::PipelineCache::default(),
            )?;

            context.device.destroy_shader_module(vertex_shader, None);
            context.device.destroy_shader_module(fragment_shader, None);

            Ok(Self {
                context,
                descriptor_handler,
                pipeline,
                pipeline_layout,
            })
        }
    }

    pub(crate) fn record_render_water(
        &self,
        command_buffer: vk::CommandBuffer,
        frame_index: usize,
        water: &Water,
        world_maps: &WorldMaps,
    ) {
        unsafe {
            self.context.device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );

            // set 0: UBO caméra
            self.context.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.descriptor_handler.world_descriptor_sets[frame_index]],
                &[],
            );

            // set 3 : cartes du monde
            self.context.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                3,
                &[world_maps.descriptor_set],
                &[],
            );

            water.mesh.bind(command_buffer);

            for node in &water.nodes {
                let push = WaterPushConstants {
                    origin: node.origin,
                    cell_size: node.size / WATER_GRID_RESOLUTION as f32,
                    sea_level: water.sea_level,
                    morph_start: node.morph_start,
                    morph_end: node.morph_end,
                };
                self.context.device.cmd_push_constants(
                    command_buffer,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    bytemuck::bytes_of(&push),
                );
                self.context.device.cmd_draw_indexed(
                    command_buffer,
                    water.mesh.index_count(),
                    1,
                    0,
                    0,
                    0,
                );
            }
        }
    }
}

impl Drop for WaterRenderSystem {
    fn drop(&mut self) {
        unsafe {
            self.context.device.destroy_pipeline(self.pipeline, None);
            self.context
                .device
                .destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}
