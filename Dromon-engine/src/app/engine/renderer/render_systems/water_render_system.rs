use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use crate::app::engine::{
    renderer::{
        descriptors::DescriptorHandler,
        render_resources::{WATER_GRID_RESOLUTION, WaterVertex},
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
            let vertex_shader = context.load_shader_module("water_vert.spv")?;
            let fragment_shader = context.load_shader_module("water_frag.spv")?;

            let push_constant_ranges = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX)
                .offset(0)
                .size(std::mem::size_of::<WaterPushConstants>() as u32)];

            let pipeline_layout = context.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    // set 0 : UBO caméra (par frame)
                    .set_layouts(&[descriptor_handler.world_descriptor_set_layout])
                    .push_constant_ranges(&push_constant_ranges),
                None,
            )?;

            let pipeline = context.create_water_pipeline(
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

            water.mesh.bind(command_buffer);

            for node in &water.nodes {
                let push = WaterPushConstants {
                    origin: node.origin,
                    cell_size: node.size / WATER_GRID_RESOLUTION as f32,
                    sea_level: water.sea_level,
                };
                self.context.device.cmd_push_constants(
                    command_buffer,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
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
