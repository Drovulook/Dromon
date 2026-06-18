mod buffer;
mod descriptors;
pub mod model;
mod swapchain;
mod texture;
mod uniform_buffer;

use crate::app::engine::renderer::descriptors::DescriptorHandler;
use crate::app::engine::renderer::model::{Model, TRIANGLE_INDICES, TRIANGLE_VERTICES, Vertex};
use crate::app::engine::renderer::uniform_buffer::UniformBuffer;
use crate::app::engine::rendering_context::ImageLayoutState;
use crate::app::engine::rendering_context::RenderingContext;
use crate::app::engine::timer::Timer;
use crate::app::logger::Logger;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;
use texture::TextureHandler;
use winit::window::Window;

struct Frame {
    command_buffer: vk::CommandBuffer,
    in_flight_fence: vk::Fence,
    uniform_buffer: UniformBuffer,
}

pub struct Renderer {
    in_flight_frames_count: usize,
    frame_index: usize,
    frames: Vec<Frame>,
    image_available_semaphores: Vec<vk::Semaphore>,
    render_finished_semaphores: Vec<vk::Semaphore>,
    acquire_semaphore_index: usize,
    frame_command_pool: vk::CommandPool,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    swapchain: swapchain::Swapchain,
    context: Arc<RenderingContext>,
    triangle_model: Model,
    descriptor_handler: DescriptorHandler,
    texture_handler: TextureHandler,
}

const SHADERS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/res/shaders/");

pub fn load_shader_module(context: &RenderingContext, path: &str) -> Result<vk::ShaderModule> {
    let code = std::fs::read(format!("{}{}", SHADERS_DIR, path))?;
    context.create_shader_module(&code)
}

impl Renderer {
    pub(crate) fn new(
        context: Arc<RenderingContext>,
        window: Arc<Window>,
        logger: Arc<Logger>,
    ) -> Result<Self> {
        let mut swapchain =
            swapchain::Swapchain::new(context.clone(), window.clone(), logger.clone())?;
        swapchain.update_size()?;

        unsafe {
            let frame_command_pool = context.device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(context.queue_families.graphics)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?;

            let in_flight_frames_count = 2;
            let command_buffers = context.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(frame_command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(in_flight_frames_count as u32),
            )?;

            let mut frames = Vec::with_capacity(command_buffers.len());
            for &command_buffer in command_buffers.iter() {
                let in_flight_fence = context.device.create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )?;
                let uniform_buffer = uniform_buffer::UniformBuffer::new(context.clone())?;
                frames.push(Frame {
                    command_buffer,
                    in_flight_fence,
                    uniform_buffer,
                });
            }

            // creating descriptor sets
            let uniform_buffer_handles: Vec<vk::Buffer> = frames
                .iter()
                .map(|frame| frame.uniform_buffer.get_handle())
                .collect();
            let descriptor_handler =
                DescriptorHandler::new(context.clone(), uniform_buffer_handles)?;

            let image_count = swapchain.images.len();

            // N+1 sémaphores en rotation : garantit qu'on ne réutilise pas un sémaphore
            // encore tenu par WSI (image X non ré-acquise depuis sa dernière présentation)
            let mut image_available_semaphores = Vec::with_capacity(image_count + 1);
            for _ in 0..image_count + 1 {
                image_available_semaphores.push(
                    context
                        .device
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
                );
            }

            // N sémaphores indexés par image_index : réutilisables dès que l'image est ré-acquise
            let mut render_finished_semaphores = Vec::with_capacity(image_count);
            for _ in 0..image_count {
                render_finished_semaphores.push(
                    context
                        .device
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
                );
            }

            let vertex_shader = load_shader_module(context.as_ref(), "vert.spv")?;
            let fragment_shader = load_shader_module(context.as_ref(), "frag.spv")?;
            let pipeline_layout = context.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[descriptor_handler.ubo_descriptor_set_layout]),
                None,
            )?;

            let pipeline = context.create_graphics_pipeline(
                vertex_shader,
                fragment_shader,
                pipeline_layout,
                &[Vertex::get_binding_description()],
                &Vertex::get_attribute_descriptions(),
                swapchain.extent,
                swapchain.format,
                vk::PipelineCache::default(),
            )?;

            context.device.destroy_shader_module(vertex_shader, None);
            context.device.destroy_shader_module(fragment_shader, None);

            let triangle_model = Model::new(
                context.clone(),
                TRIANGLE_VERTICES.to_vec(),
                TRIANGLE_INDICES.to_vec(),
            )?;

            let texture_handler = TextureHandler::new(context.clone(), logger.clone())?;

            Renderer::initialize(context.clone(), &triangle_model, &texture_handler)?;

            Ok(Self {
                in_flight_frames_count,
                frame_index: 0,
                frames,
                image_available_semaphores,
                render_finished_semaphores,
                acquire_semaphore_index: 0,
                frame_command_pool,
                pipeline,
                pipeline_layout,
                swapchain,
                context,
                triangle_model,
                descriptor_handler,
                texture_handler,
            })
        }
    }

    fn initialize(
        context: Arc<RenderingContext>,
        model: &Model,
        texture_handler: &TextureHandler,
    ) -> Result<()> {
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
        model.copy_from_staging_to_device(&transfer_command_buffer);

        context.transition_image_layout(
            transfer_command_buffer,
            &[(
                texture_handler.texture_image,
                ImageLayoutState::default(),
                ImageLayoutState {
                    access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                    layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    stage_mask: vk::PipelineStageFlags2::COPY,
                    queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                },
            )],
        );

        texture_handler.copy_buffer_to_image(&transfer_command_buffer)?;

        context.transition_image_layout(
            transfer_command_buffer,
            &[(
                texture_handler.texture_image,
                ImageLayoutState {
                    access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                    layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    stage_mask: vk::PipelineStageFlags2::COPY,
                    queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                },
                ImageLayoutState {
                    access_mask: vk::AccessFlags2::SHADER_READ,
                    layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    stage_mask: vk::PipelineStageFlags2::FRAGMENT_SHADER,
                    queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                },
            )],
        );
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

    pub fn resize(&mut self) {
        self.swapchain.is_dirty = true;
    }

    pub fn render(&mut self, timer: &Timer) -> Result<()> {
        let _ = timer; // utilisé bientôt pour animer l'UBO
        let frame = &self.frames[self.frame_index];

        unsafe {
            self.context
                .device
                .wait_for_fences(&[frame.in_flight_fence], true, u64::MAX)?;

            if self.swapchain.is_dirty {
                self.swapchain.update_size()?;
            }

            if self.swapchain.extent.width == 0 || self.swapchain.extent.height == 0 {
                return Ok(());
            }

            self.context.device.reset_fences(&[frame.in_flight_fence])?;
            self.context
                .device
                .reset_command_buffer(frame.command_buffer, vk::CommandBufferResetFlags::empty())?;

            let image_available_semaphore =
                self.image_available_semaphores[self.acquire_semaphore_index];
            self.acquire_semaphore_index =
                (self.acquire_semaphore_index + 1) % self.image_available_semaphores.len();

            let image_index = self
                .swapchain
                .acquire_next_image(image_available_semaphore)?;

            let render_finished_semaphore = self.render_finished_semaphores[image_index as usize];

            let undefined_image_state = ImageLayoutState {
                access_mask: vk::AccessFlags2::empty(),
                layout: vk::ImageLayout::UNDEFINED,
                stage_mask: vk::PipelineStageFlags2::TOP_OF_PIPE,
                queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            };

            let renderable_image_state = ImageLayoutState {
                access_mask: vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            };

            let present_image_state = ImageLayoutState {
                access_mask: vk::AccessFlags2::empty(),
                layout: vk::ImageLayout::PRESENT_SRC_KHR,
                stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            };

            // Commands
            self.context.device.begin_command_buffer(
                frame.command_buffer,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;

            self.context.transition_image_layout(
                frame.command_buffer,
                &[(
                    self.swapchain.images[image_index as usize],
                    undefined_image_state,
                    renderable_image_state,
                )],
            );

            self.context.begin_rendering(
                frame.command_buffer,
                self.swapchain.image_views[image_index as usize],
                vk::ClearColorValue {
                    float32: [0.0015, 0.0, 0.0015, 1.0],
                },
                vk::Rect2D::default().extent(self.swapchain.extent),
            );

            self.context.device.cmd_set_viewport(
                frame.command_buffer,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: self.swapchain.extent.width as f32,
                    height: self.swapchain.extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );

            self.context.device.cmd_set_scissor(
                frame.command_buffer,
                0,
                &[vk::Rect2D::default().extent(self.swapchain.extent)],
            );

            self.context.device.cmd_bind_pipeline(
                frame.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );

            self.triangle_model.bind_index_buffer(frame.command_buffer);
            self.triangle_model.bind_vertex_buffer(frame.command_buffer);
            frame.uniform_buffer.update(
                timer,
                self.swapchain.extent.width as f32,
                self.swapchain.extent.height as f32,
            );

            self.context.device.cmd_bind_descriptor_sets(
                frame.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.descriptor_handler.ubo_descriptor_sets[self.frame_index]],
                &[],
            );

            self.context.device.cmd_draw_indexed(
                frame.command_buffer,
                self.triangle_model.indices.len() as u32,
                1,
                0,
                0,
                0,
            );

            self.context.device.cmd_end_rendering(frame.command_buffer);

            self.context.transition_image_layout(
                frame.command_buffer,
                &[(
                    self.swapchain.images[image_index as usize],
                    renderable_image_state,
                    present_image_state,
                )],
            );

            self.context
                .device
                .end_command_buffer(frame.command_buffer)?;
            // End on commands

            self.context.device.queue_submit(
                self.context.queues[self.context.queue_families.graphics as usize],
                &[vk::SubmitInfo::default()
                    .command_buffers(&[frame.command_buffer])
                    .wait_semaphores(&[image_available_semaphore])
                    .wait_dst_stage_mask(&[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT])
                    .signal_semaphores(&[render_finished_semaphore])],
                frame.in_flight_fence,
            )?;

            self.swapchain
                .present(image_index, render_finished_semaphore)?;

            self.frame_index = (self.frame_index + 1) % self.in_flight_frames_count;
        }

        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.context.device.device_wait_idle();
            self.frames.drain(..).for_each(|frame| {
                self.context
                    .device
                    .destroy_fence(frame.in_flight_fence, None);
            });
            for semaphore in self.image_available_semaphores.drain(..) {
                self.context.device.destroy_semaphore(semaphore, None);
            }
            for semaphore in self.render_finished_semaphores.drain(..) {
                self.context.device.destroy_semaphore(semaphore, None);
            }
            self.context
                .device
                .destroy_command_pool(self.frame_command_pool, None);
            self.context.device.destroy_pipeline(self.pipeline, None);
            self.context
                .device
                .destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}
