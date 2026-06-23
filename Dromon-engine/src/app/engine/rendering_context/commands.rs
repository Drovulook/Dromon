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
        mip_levels: u32,
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
                            .level_count(mip_levels)
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

    pub fn generate_mipmaps(
        &self,
        command_buffer: vk::CommandBuffer,
        image: &vk::Image,
        image_format: vk::Format,
        text_width: u32,
        text_height: u32,
        mip_levels: u32,
    ) -> Result<()> {
        let props = unsafe {
            self.instance
                .get_physical_device_format_properties(self.physical_device.handle, image_format)
        };

        if !props
            .optimal_tiling_features
            .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR)
        {
            return Err(anyhow::anyhow!(
                "texture image format does not support linear blitting"
            ));
        }

        let mut barrier = vk::ImageMemoryBarrier2::default()
            .image(*image)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1),
            );

        let mut mip_width = text_width as i32;
        let mut mip_height = text_height as i32;

        for i in 1..mip_levels {
            // transition to TRANSFER_SRC_OPTIMAL
            barrier.subresource_range.base_mip_level = i - 1;
            barrier.old_layout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
            barrier.new_layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
            barrier.src_access_mask = vk::AccessFlags2::TRANSFER_WRITE;
            barrier.dst_access_mask = vk::AccessFlags2::TRANSFER_READ;
            barrier.src_stage_mask = vk::PipelineStageFlags2::TRANSFER;
            barrier.dst_stage_mask = vk::PipelineStageFlags2::TRANSFER;
            unsafe {
                self.device.cmd_pipeline_barrier2(
                    command_buffer,
                    &vk::DependencyInfo::default().image_memory_barriers(&[barrier]),
                );
            }

            // blit
            let blit = vk::ImageBlit::default()
                .src_offsets([
                    vk::Offset3D { x: 0, y: 0, z: 0 },
                    vk::Offset3D {
                        x: mip_width,
                        y: mip_height,
                        z: 1,
                    },
                ])
                .dst_offsets([
                    vk::Offset3D { x: 0, y: 0, z: 0 },
                    vk::Offset3D {
                        x: if mip_width > 1 { mip_width / 2 } else { 1 },
                        y: if mip_height > 1 { mip_height / 2 } else { 1 },
                        z: 1,
                    },
                ])
                .src_subresource(
                    vk::ImageSubresourceLayers::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .mip_level(i - 1)
                        .base_array_layer(0)
                        .layer_count(1),
                )
                .dst_subresource(
                    vk::ImageSubresourceLayers::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .mip_level(i)
                        .base_array_layer(0)
                        .layer_count(1),
                );
            unsafe {
                self.device.cmd_blit_image(
                    command_buffer,
                    *image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    *image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[blit],
                    vk::Filter::LINEAR,
                );
            }

            // transition to SHADER_READ_ONLY_OPTIMAL
            barrier.old_layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
            barrier.new_layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
            barrier.src_access_mask = vk::AccessFlags2::TRANSFER_READ;
            barrier.dst_access_mask = vk::AccessFlags2::SHADER_READ;
            barrier.src_stage_mask = vk::PipelineStageFlags2::TRANSFER;
            barrier.dst_stage_mask = vk::PipelineStageFlags2::FRAGMENT_SHADER;
            unsafe {
                self.device.cmd_pipeline_barrier2(
                    command_buffer,
                    &vk::DependencyInfo::default().image_memory_barriers(&[barrier]),
                );
            }
            if mip_width > 1 {
                mip_width /= 2;
            }
            if mip_height > 1 {
                mip_height /= 2;
            }
        }

        // transition to SHADER_READ_ONLY_OPTIMAL for the last mip level
        barrier.subresource_range.base_mip_level = mip_levels - 1;
        barrier.old_layout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
        barrier.new_layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        barrier.src_access_mask = vk::AccessFlags2::TRANSFER_WRITE;
        barrier.dst_access_mask = vk::AccessFlags2::SHADER_READ;
        barrier.src_stage_mask = vk::PipelineStageFlags2::TRANSFER;
        barrier.dst_stage_mask = vk::PipelineStageFlags2::FRAGMENT_SHADER;
        unsafe {
            self.device.cmd_pipeline_barrier2(
                command_buffer,
                &vk::DependencyInfo::default().image_memory_barriers(&[barrier]),
            );
        }

        Ok(())
    }
}
