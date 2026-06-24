mod buffer;
mod descriptors;
mod render_object;
mod swapchain;
mod uniform_buffer;

use crate::app::engine::renderer::descriptors::DescriptorHandler;
use crate::app::engine::renderer::render_object::{RenderObject, RenderObjectResourceManager};
use crate::app::engine::renderer::render_object::{Transform, Vertex};
use crate::app::engine::renderer::uniform_buffer::UniformBuffer;
use crate::app::engine::rendering_context::ImageLayoutState;
use crate::app::engine::rendering_context::RenderingContext;
use crate::app::engine::timer::Timer;
use crate::app::logger::Logger;
use anyhow::{Context, Result};
use ash::vk;
use std::sync::Arc;
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
    descriptor_handler: Arc<DescriptorHandler>,
    render_object_resource_manager: render_object::RenderObjectResourceManager,
    render_object_list: Vec<RenderObject>,
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
            let descriptor_handler = Arc::new(DescriptorHandler::new(
                context.clone(),
                uniform_buffer_handles,
            )?);

            ////////// 3D models //////////
            const GLTF_PATH: &str = "/res/models/dromon_ship/scene.gltf";
            const TEXTURE_PATH: &str = "/res/models/dromon_ship/DefaultMaterial_baseColor.png";

            let mut rorm = render_object::RenderObjectResourceManager::new(
                context.clone(),
                logger.clone(),
                descriptor_handler.clone(),
            )?;
            rorm.add_texture("texture1".to_string(), TEXTURE_PATH.to_string())?;
            rorm.add_texture("texture2".to_string(), TEXTURE_PATH.to_string())?;
            rorm.add_mesh("mesh1".to_string(), GLTF_PATH.to_string())?;
            rorm.add_mesh("mesh2".to_string(), GLTF_PATH.to_string())?;

            let mut render_object_list = Vec::new();

            render_object_list.push(RenderObject::new(
                rorm.get_mesh("mesh1")
                    .context("mesh \"mesh1\" introuvable")?,
                rorm.get_texture("texture1")
                    .context("texture \"texture1\" introuvable")?,
                Transform {
                    translation: glam::Vec3::new(0.0, 1.0, 0.0),
                    ..Default::default()
                },
            ));
            render_object_list.push(RenderObject::new(
                rorm.get_mesh("mesh2")
                    .context("mesh \"mesh2\" introuvable")?,
                rorm.get_texture("texture2")
                    .context("texture \"texture2\" introuvable")?,
                Transform {
                    translation: glam::Vec3::new(0.0, -1.0, 0.0),
                    ..Default::default()
                },
            ));

            let image_count = swapchain.color_images.len();

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
            // Push constant : la matrice « model » (Mat4 = 64 octets), poussée
            // par objet dans le command buffer. Visible côté vertex shader.
            let push_constant_ranges = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX)
                .offset(0)
                .size(std::mem::size_of::<glam::Mat4>() as u32)];

            let pipeline_layout = context.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[
                        // set 0 : UBO caméra (par frame)
                        descriptor_handler.world_descriptor_set_layout,
                        // set 1 : texture (par matériau) — toutes les textures partagent
                        // CE layout ; on bind un set différent par objet au moment du draw.
                        descriptor_handler.texture_descriptor_set_layout,
                    ])
                    .push_constant_ranges(&push_constant_ranges),
                None,
            )?;

            let pipeline = context.create_graphics_pipeline(
                vertex_shader,
                fragment_shader,
                pipeline_layout,
                &[Vertex::get_binding_description()],
                &Vertex::get_attribute_descriptions(),
                swapchain.extent,
                swapchain.color_format,
                swapchain.depth_format,
                context.get_max_usable_sample_count(),
                vk::PipelineCache::default(),
            )?;

            context.device.destroy_shader_module(vertex_shader, None);
            context.device.destroy_shader_module(fragment_shader, None);

            Renderer::initialize(context.clone(), &rorm)?;

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
                descriptor_handler,
                render_object_resource_manager: rorm,
                render_object_list,
            })
        }
    }

    fn initialize(
        context: Arc<RenderingContext>,
        rorm: &RenderObjectResourceManager,
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
        rorm.initialize(&transfer_command_buffer)?;
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

            // color image
            let undefined_color_image_state = ImageLayoutState {
                access_mask: vk::AccessFlags2::empty(),
                layout: vk::ImageLayout::UNDEFINED,
                stage_mask: vk::PipelineStageFlags2::TOP_OF_PIPE,
                queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            };
            let renderable_color_image_state = ImageLayoutState {
                access_mask: vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            };
            let present_color_image_state = ImageLayoutState {
                access_mask: vk::AccessFlags2::empty(),
                layout: vk::ImageLayout::PRESENT_SRC_KHR,
                stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            };

            // depth image
            let undefined_depth_image_state = ImageLayoutState {
                access_mask: vk::AccessFlags2::empty(),
                layout: vk::ImageLayout::UNDEFINED,
                stage_mask: vk::PipelineStageFlags2::TOP_OF_PIPE,
                queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            };
            let renderable_depth_image_state = ImageLayoutState {
                access_mask: vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
                layout: vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                stage_mask: vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS,
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
                &[
                    (
                        // image MSAA : cible de rendu (attachment couleur principal)
                        self.swapchain.msaa_color_image,
                        undefined_color_image_state,
                        renderable_color_image_state,
                    ),
                    (
                        // image swapchain : cible de resolve
                        self.swapchain.color_images[image_index as usize],
                        undefined_color_image_state,
                        renderable_color_image_state,
                    ),
                ],
                1,
            );

            self.context.transition_image_layout(
                frame.command_buffer,
                &[(
                    self.swapchain.depth_image,
                    undefined_depth_image_state,
                    renderable_depth_image_state,
                )],
                1,
            );

            self.context.begin_rendering(
                frame.command_buffer,
                self.swapchain.msaa_color_image_view,
                self.swapchain.color_image_views[image_index as usize],
                self.swapchain.depth_image_view,
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

            frame.uniform_buffer.update(
                timer,
                self.swapchain.extent.width as f32,
                self.swapchain.extent.height as f32,
            );

            // set 0: UBO caméra
            self.context.device.cmd_bind_descriptor_sets(
                frame.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.descriptor_handler.world_descriptor_sets[self.frame_index]],
                &[],
            );

            for render_object in &self.render_object_list {
                // set 1 : la texture de CET objet
                self.context.device.cmd_bind_descriptor_sets(
                    frame.command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline_layout,
                    1, // first_set = 1
                    &[render_object.texture.descriptor_set],
                    &[],
                );
                // push constant : la matrice « model » de CET objet (transform → Mat4).
                let spin = glam::Mat4::from_rotation_z(timer.elapsed_secs() * 0.6);
                let model_matrix = render_object.transform.to_matrix() * spin;
                self.context.device.cmd_push_constants(
                    frame.command_buffer,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
                    0,
                    bytemuck::bytes_of(&model_matrix),
                );
                render_object.mesh.bind(frame.command_buffer);

                self.context.device.cmd_draw_indexed(
                    frame.command_buffer,
                    render_object.mesh.indices.len() as u32,
                    1,
                    0,
                    0,
                    0,
                );
            }

            self.context.device.cmd_end_rendering(frame.command_buffer);

            self.context.transition_image_layout(
                frame.command_buffer,
                &[(
                    self.swapchain.color_images[image_index as usize],
                    renderable_color_image_state,
                    present_color_image_state,
                )],
                1,
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
