mod buffer;
mod camera;
mod descriptors;
pub mod image_layout_state;
pub(crate) mod render_resources;
mod render_systems;
mod renderer_initialize;
mod renderer_record_pass;
mod shadow_map;
mod swapchain;
mod uniform_buffer;
pub(crate) mod world;

use crate::app::engine::inputs::InputState;
use crate::app::engine::renderer::descriptors::DescriptorHandler;
use crate::app::engine::renderer::render_systems::TerrainRenderSystem;
use crate::app::engine::renderer::shadow_map::ShadowMap;
use crate::app::engine::renderer::uniform_buffer::UniformBuffer;
use crate::app::engine::renderer::world::World;
use crate::app::engine::rendering_context::RenderingContext;
use crate::app::engine::timer::Timer;
use crate::app::logger::Logger;
use crate::{Scene, app::engine::renderer::render_systems::ObjectRenderSystem};
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
    object_render_system: ObjectRenderSystem,
    terrain_render_sytem: TerrainRenderSystem,
    shadow_map: ShadowMap,
    swapchain: swapchain::Swapchain,
    context: Arc<RenderingContext>,
    descriptor_handler: Arc<DescriptorHandler>,
    world: World,
}

impl Renderer {
    pub(crate) fn new(
        context: Arc<RenderingContext>,
        window: Arc<Window>,
        logger: Arc<Logger>,
        scene: &mut dyn Scene,
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

            let mut world =
                World::new(logger.clone(), context.clone(), descriptor_handler.clone())?;

            // La scène peuple le monde (assets + RenderObject) AVANT l'upload GPU
            // (`Renderer::initialize` plus bas copie ces données vers le device).
            scene.setup(&mut world)?;

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

            let object_render_system = ObjectRenderSystem::new(
                context.clone(),
                descriptor_handler.clone(),
                swapchain.color_format,
                swapchain.depth_format,
                swapchain.msaa_samples,
                swapchain.extent,
                shadow_map.format,
            )?;

            let terrain_render_sytem = TerrainRenderSystem::new(
                context.clone(),
                descriptor_handler.clone(),
                swapchain.color_format,
                swapchain.depth_format,
                swapchain.msaa_samples,
                swapchain.extent,
                shadow_map.format,
            )?;

            Renderer::initialize(context.clone(), &world)?;

            Ok(Self {
                in_flight_frames_count,
                frame_index: 0,
                frames,
                image_available_semaphores,
                render_finished_semaphores,
                acquire_semaphore_index: 0,
                frame_command_pool,
                object_render_system,
                terrain_render_sytem,
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

    pub fn render(
        &mut self,
        timer: &Timer,
        input_state: &InputState,
        scene: &mut dyn Scene,
    ) -> Result<()> {
        // ratio largeur/hauteur de la fenêtre, pour la projection de la caméra.
        // `.max(1)` évite une division par zéro quand la fenêtre est minimisée.
        let aspect =
            self.swapchain.extent.width as f32 / self.swapchain.extent.height.max(1) as f32;
        self.world.update_world_data(timer, input_state, aspect);
        // La logique de jeu (déplacement/rotation des objets) est déléguée à la scène.
        scene.update(&mut self.world, timer);

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
        }
    }
}
