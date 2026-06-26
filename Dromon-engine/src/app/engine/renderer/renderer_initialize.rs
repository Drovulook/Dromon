use crate::app::engine::{
    renderer::{Renderer, world::World},
    rendering_context::RenderingContext,
};
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

impl Renderer {
    pub(super) fn initialize(context: Arc<RenderingContext>, world: &World) -> Result<()> {
        //create transfer command pool and buffer
        let transfer_command_pool = unsafe {
            context.device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    // TODO : utiliser la famille de commandes appropriée
                    .queue_family_index(context.queue_families.graphics)
                    .flags(vk::CommandPoolCreateFlags::TRANSIENT),
                None,
            )
        }?;
        let transfer_command_buffer = unsafe {
            context.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(transfer_command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
        }?[0];

        unsafe {
            context.device.begin_command_buffer(
                transfer_command_buffer,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
        }?;

        ////////// all transfer commands go here
        world.initialize(&transfer_command_buffer)?;
        //////////

        unsafe { context.device.end_command_buffer(transfer_command_buffer) }?;

        // submit
        let graphics_queue = context.queues[context.queue_families.graphics as usize];
        unsafe {
            context.device.queue_submit(
                graphics_queue,
                &[vk::SubmitInfo::default().command_buffers(&[transfer_command_buffer])],
                vk::Fence::null(),
            )
        }?;
        unsafe { context.device.queue_wait_idle(graphics_queue) }?;

        // cleanup
        unsafe {
            context
                .device
                .destroy_command_pool(transfer_command_pool, None)
        };

        Ok(())
    }
}
