mod buffer;
mod camera;
mod descriptors;
pub mod image_layout_state;
mod render_object;
mod renderer_initialize;
mod renderer_record_pass;
mod shadow_map;
mod swapchain;
mod uniform_buffer;
mod world;

use crate::app::engine::inputs::InputState;
use crate::app::engine::renderer::descriptors::DescriptorHandler;
use crate::app::engine::renderer::image_layout_state::ImageLayoutState;
use crate::app::engine::renderer::render_object::Vertex;
use crate::app::engine::renderer::shadow_map::ShadowMap;
use crate::app::engine::renderer::uniform_buffer::UniformBuffer;
use crate::app::engine::renderer::world::World;
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
    // Passe d'ombre : sa pipeline depth-only et l'image cible. La pipeline réutilise
    // le `pipeline_layout` principal (compatible : même set 0 + push constants).
    shadow_pipeline: vk::Pipeline,
    shadow_map: ShadowMap,
    swapchain: swapchain::Swapchain,
    context: Arc<RenderingContext>,
    descriptor_handler: Arc<DescriptorHandler>,
    world: World,
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

            // Shadow map : créée avant les descriptor sets car son image view et son
            // sampler sont écrits dans le descriptor set 2 dès la construction.
            let shadow_map = ShadowMap::new(context.clone())?;

            // creating descriptor sets
            let uniform_buffer_handles: Vec<vk::Buffer> = frames
                .iter()
                .map(|frame| frame.uniform_buffer.get_handle())
                .collect();
            let descriptor_handler = Arc::new(DescriptorHandler::new(
                context.clone(),
                uniform_buffer_handles,
                shadow_map.image_view,
                shadow_map.sampler,
            )?);

            let world = World::new(logger.clone(), context.clone(), descriptor_handler.clone())?;

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
            // Push constant : deux Mat4 (2 × 64 = 128 octets, la taille minimale
            // garantie par Vulkan), poussées par objet. La 1re est la matrice
            // « model » ; la 2de est la matrice normale (inverse-transposée de
            // model, calculée côté CPU car Slang n'a pas inverse()).
            let push_constant_ranges = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::VERTEX)
                .offset(0)
                .size(2 * std::mem::size_of::<glam::Mat4>() as u32)];

            let pipeline_layout = context.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[
                        // set 0 : UBO caméra (par frame)
                        descriptor_handler.world_descriptor_set_layout,
                        // set 1 : texture (par matériau) — toutes les textures partagent
                        // CE layout ; on bind un set différent par objet au moment du draw.
                        descriptor_handler.texture_descriptor_set_layout,
                        // set 2 : shadow map (unique, partagée par toutes les frames)
                        descriptor_handler.shadow_descriptor_set_layout,
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

            // Pipeline de la passe d'ombre : réutilise le pipeline_layout principal
            // (set 0 + push constants suffisent au shadow vertex shader). Même format
            // de profondeur que la shadow map.
            let shadow_vertex_shader = load_shader_module(context.as_ref(), "shadow_vert.spv")?;
            let shadow_pipeline = context.create_shadow_pipeline(
                shadow_vertex_shader,
                pipeline_layout,
                &[Vertex::get_binding_description()],
                &Vertex::get_attribute_descriptions(),
                shadow_map.format,
                vk::PipelineCache::default(),
            )?;
            context
                .device
                .destroy_shader_module(shadow_vertex_shader, None);

            Renderer::initialize(context.clone(), &world)?;

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
                shadow_pipeline,
                shadow_map,
                swapchain,
                context,
                descriptor_handler,
                world,
            })
        }
    }

    pub fn resize(&mut self) {
        self.swapchain.is_dirty = true;
    }

    pub fn render(&mut self, timer: &Timer, input_state: &InputState) -> Result<()> {
        // ratio largeur/hauteur de la fenêtre, pour la projection de la caméra.
        // `.max(1)` évite une division par zéro quand la fenêtre est minimisée.
        let aspect =
            self.swapchain.extent.width as f32 / self.swapchain.extent.height.max(1) as f32;
        self.world.update_world_data(timer, input_state, aspect);
        self.world.update_render_objects(timer);

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

            // Commands
            self.context.device.begin_command_buffer(
                frame.command_buffer,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;

            // L'UBO est mis à jour AVANT toute passe : la passe d'ombre comme la
            // passe principale lisent la même `light_view_proj` depuis le set 0.
            frame.uniform_buffer.update(
                self.world.camera.view,
                self.world.camera.proj,
                self.world.light.view_proj(),
                self.world.light.direction,
                self.world.light.color,
                self.world.light.intensity,
            );

            // 1re passe : on remplit la shadow map depuis le point de vue de la lumière.
            self.record_shadow_pass(frame.command_buffer, self.frame_index);

            // 2nde passe : on dessine le monde.
            self.record_render_pass(frame, image_index);

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
                .destroy_pipeline(self.shadow_pipeline, None);
            self.context
                .device
                .destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}
