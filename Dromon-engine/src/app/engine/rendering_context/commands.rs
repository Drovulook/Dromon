use crate::app::engine::rendering_context::ImageLayoutState;

use super::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

impl RenderingContext {
    pub fn begin_rendering(
        &self,
        command_buffer: vk::CommandBuffer,
        image_view: vk::ImageView,
        depth_image_view: vk::ImageView,
        clear_color: vk::ClearColorValue,
        render_area: vk::Rect2D,
    ) {
        unsafe {
            self.device.cmd_begin_rendering(
                command_buffer,
                &vk::RenderingInfo::default()
                    .layer_count(1)
                    .color_attachments(&[vk::RenderingAttachmentInfo::default()
                        .image_view(image_view)
                        .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                        .clear_value(vk::ClearValue { color: clear_color })
                        .load_op(vk::AttachmentLoadOp::CLEAR)
                        .store_op(vk::AttachmentStoreOp::STORE)])
                    .depth_attachment(
                        &vk::RenderingAttachmentInfo::default()
                            .image_view(depth_image_view)
                            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                            .clear_value(vk::ClearValue {
                                depth_stencil: vk::ClearDepthStencilValue {
                                    depth: 1.0,
                                    stencil: 0,
                                },
                            })
                            .load_op(vk::AttachmentLoadOp::CLEAR)
                            .store_op(vk::AttachmentStoreOp::DONT_CARE),
                    )
                    .render_area(render_area),
            );
        }
    }

    pub fn transition_image_layout(
        &self,
        command_buffer: vk::CommandBuffer,
        states: &[(vk::Image, ImageLayoutState, ImageLayoutState)],
    ) {
        let barriers = states
            .iter()
            .map(|(image, old_layout, new_layout)| {
                let aspect_flag = if new_layout.layout == vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL
                {
                    vk::ImageAspectFlags::DEPTH
                } else {
                    vk::ImageAspectFlags::COLOR
                };
                vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(old_layout.stage_mask)
                    .dst_stage_mask(new_layout.stage_mask)
                    .src_access_mask(old_layout.access_mask)
                    .dst_access_mask(new_layout.access_mask)
                    .old_layout(old_layout.layout)
                    .new_layout(new_layout.layout)
                    .src_queue_family_index(old_layout.queue_family_index)
                    .dst_queue_family_index(new_layout.queue_family_index)
                    .image(*image)
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(aspect_flag)
                            .base_mip_level(0)
                            .level_count(1)
                            .base_array_layer(0)
                            .layer_count(1),
                    )
            })
            .collect::<Vec<_>>();

        unsafe {
            self.device.cmd_pipeline_barrier2(
                command_buffer,
                &vk::DependencyInfo::default().image_memory_barriers(&barriers),
            );
        }
    }
}
