use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use crate::app::engine::{
    renderer::descriptors::DescriptorHandler, rendering_context::RenderingContext,
};

pub(crate) struct SkyRenderSystem {
    context: Arc<RenderingContext>,
    descriptor_handler: Arc<DescriptorHandler>,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
}

impl SkyRenderSystem {
    pub(crate) fn new(
        context: Arc<RenderingContext>,
        descriptor_handler: Arc<DescriptorHandler>,
        color_format: vk::Format,
        depth_format: vk::Format,
        msaa_samples: vk::SampleCountFlags,
    ) -> Result<Self> {
        unsafe {
            let vertex_shader = context.load_shader_module("sky_vert.spv")?;
            let fragment_shader = context.load_shader_module("sky_frag.spv")?;

            let pipeline_layout = context.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&[
                    // set 0 : UBO caméra (par frame)
                    descriptor_handler.world_descriptor_set_layout,
                ]),
                None,
            )?;

            let pipeline = context.create_sky_pipeline(
                vertex_shader,
                fragment_shader,
                pipeline_layout,
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

    pub(crate) fn record_render_sky(&self, command_buffer: vk::CommandBuffer, frame_index: usize) {
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

            // 3 sommets générés par le vertex shader (SV_VertexID), sans vertex buffer.
            self.context.device.cmd_draw(command_buffer, 3, 1, 0, 0);
        }
    }
}

impl Drop for SkyRenderSystem {
    fn drop(&mut self) {
        unsafe {
            self.context.device.destroy_pipeline(self.pipeline, None);
            self.context
                .device
                .destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}
