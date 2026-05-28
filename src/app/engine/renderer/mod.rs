mod swapchain;

use crate::app::engine::rendering_context::RenderingContext;
use anyhow::{Result, anyhow};
use ash::vk;
use softbuffer::Context as SoftBufferContext;
use softbuffer::Surface;
use std::io;
use std::{num::NonZeroU32, sync::Arc};
use winit::window::Window;

struct Frame {
    command_buffer: vk::CommandBuffer,
    image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
    in_flight_fence: vk::Fence,
}

pub struct Renderer {
    surface: Surface<Arc<Window>, Arc<Window>>,
    frame_index: usize,
    frames: Vec<Frame>,
    command_pool: vk::CommandPool,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    swapchain: swapchain::Swapchain,
    context: Arc<RenderingContext>,
}

const SHADERS_DIR: &str = "res/shaders/";

pub fn load_shader_module(context: &RenderingContext, path: &str) -> Result<vk::ShaderModule> {
    let code = std::fs::read(format!("{}{}", SHADERS_DIR, path))?;
    context.create_shader_module(&code)
}

impl Renderer {
    pub(crate) fn new(context: Arc<RenderingContext>, window: Arc<Window>) -> Result<Self> {
        let softbuffer_context =
            SoftBufferContext::new(window.clone()).map_err(|e| anyhow!("{e}"))?;
        let surface =
            Surface::new(&softbuffer_context, window.clone()).map_err(|e| anyhow!("{e}"))?;
        let mut swapchain = swapchain::Swapchain::new(context.clone(), window.clone())?;
        swapchain.update_size()?;

        let vertex_shader = load_shader_module(context.as_ref(), "vert.spv")?;
        let fragment_shader = load_shader_module(context.as_ref(), "frag.spv")?;

        unsafe {
            let pipeline_layout = context
                .device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)?;

            let pipeline = context.create_graphics_pipeline(
                vertex_shader,
                fragment_shader,
                pipeline_layout,
                swapchain.extent,
                swapchain.format,
                vk::PipelineCache::default(),
            )?;

            context.device.destroy_shader_module(vertex_shader, None);
            context.device.destroy_shader_module(fragment_shader, None);

            let command_pool = context.device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(context.queue_families.graphics)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?;

            let command_buffers = context.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(swapchain.image_views.len() as u32),
            )?;

            let mut frames = Vec::with_capacity(command_buffers.len());
            for (_, &command_buffer) in command_buffers.iter().enumerate() {
                let image_available_semaphore = context
                    .device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?;
                let render_finished_semaphore = context
                    .device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?;
                let in_flight_fence = context.device.create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )?;

                frames.push(Frame {
                    command_buffer,
                    image_available_semaphore,
                    render_finished_semaphore,
                    in_flight_fence,
                });
            }

            Ok(Self {
                frame_index: 0,
                frames,
                command_pool,
                surface,
                pipeline,
                pipeline_layout,
                swapchain,
                context,
            })
        }
    }

    pub fn resize(&mut self) -> Result<()> {
        self.swapchain.update_size()
    }

    pub(crate) fn draw(&mut self) -> Result<()> {
        let (width, height) = {
            let size = self.surface.window().inner_size();
            (size.width, size.height)
        };
        self.surface
            .resize(
                NonZeroU32::new(width).unwrap_or(NonZeroU32::new(1).unwrap()),
                NonZeroU32::new(height).unwrap_or(NonZeroU32::new(1).unwrap()),
            )
            .map_err(|e| anyhow!("{e}"))?;
        let mut buffer = self.surface.buffer_mut().map_err(|e| anyhow!("{e}"))?;
        buffer.fill(0x00000000);
        buffer.present().map_err(|e| anyhow!("{e}"))?;
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.context.device.destroy_pipeline(self.pipeline, None);
            self.context
                .device
                .destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}
