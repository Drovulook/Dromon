use crate::app::engine::renderer::{Frame, Renderer, image_layout_state::ImageLayoutState};
use anyhow::Result;
use ash::vk;

impl Renderer {
    /// Enregistre la passe d'ombre dans le command buffer : on rend la scène en
    /// depth-only depuis la lumière vers la shadow map, puis on transitionne
    /// celle-ci en lecture pour la passe principale.
    pub(super) fn record_shadow_pass(&self, command_buffer: vk::CommandBuffer, frame_index: usize) {
        // shadow map : UNDEFINED → attachment de profondeur (aspect DEPTH explicite).
        self.context.transition_image_layout_aspect(
            command_buffer,
            &[(
                self.shadow_map.image,
                ImageLayoutState::UNDEFINED_DEPTH_IMAGE_STATE,
                ImageLayoutState::SHADOW_MAP_WRITE_STATE,
            )],
            1,
            vk::ImageAspectFlags::DEPTH,
        );

        self.context.begin_shadow_rendering(
            command_buffer,
            self.shadow_map.image_view,
            vk::Rect2D::default().extent(self.shadow_map.extent),
        );

        unsafe {
            // viewport/scissor à la taille de la shadow map (≠ taille de la fenêtre).
            self.context.device.cmd_set_viewport(
                command_buffer,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: self.shadow_map.extent.width as f32,
                    height: self.shadow_map.extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            self.context.device.cmd_set_scissor(
                command_buffer,
                0,
                &[vk::Rect2D::default().extent(self.shadow_map.extent)],
            );

            self.context.device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.shadow_pipeline,
            );

            // set 0 : UBO (le shadow vertex shader y lit light_view_proj).
            self.context.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.descriptor_handler.world_descriptor_sets[frame_index]],
                &[],
            );

            for render_object in &self.world.render_objects {
                let model = render_object.transform.to_matrix();
                self.context.device.cmd_push_constants(
                    command_buffer,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
                    0,
                    bytemuck::bytes_of(&model),
                );
                render_object.mesh.bind(command_buffer);
                self.context.device.cmd_draw_indexed(
                    command_buffer,
                    render_object.mesh.indices.len() as u32,
                    1,
                    0,
                    0,
                    0,
                );
            }

            self.context.device.cmd_end_rendering(command_buffer);
        }

        // shadow map : attachment de profondeur → lecture dans le frag shader.
        self.context.transition_image_layout_aspect(
            command_buffer,
            &[(
                self.shadow_map.image,
                ImageLayoutState::SHADOW_MAP_WRITE_STATE,
                ImageLayoutState::SHADOW_MAP_READ_STATE,
            )],
            1,
            vk::ImageAspectFlags::DEPTH,
        );
    }

    pub(super) fn record_render_pass(&self, frame: &Frame, image_index: u32) {
        self.context.transition_image_layout(
            frame.command_buffer,
            &[
                (
                    // image MSAA : cible de rendu (attachment couleur principal)
                    self.swapchain.msaa_color_image,
                    ImageLayoutState::UNDEFINED_COLOR_IMAGE_STATE,
                    ImageLayoutState::RENDERABLE_COLOR_IMAGE_STATE,
                ),
                (
                    // image swapchain : cible de resolve
                    self.swapchain.color_images[image_index as usize],
                    ImageLayoutState::UNDEFINED_COLOR_IMAGE_STATE,
                    ImageLayoutState::RENDERABLE_COLOR_IMAGE_STATE,
                ),
            ],
            1,
        );

        self.context.transition_image_layout(
            frame.command_buffer,
            &[(
                self.swapchain.depth_image,
                ImageLayoutState::UNDEFINED_DEPTH_IMAGE_STATE,
                ImageLayoutState::RENDERABLE_DEPTH_IMAGE_STATE,
            )],
            1,
        );

        unsafe {
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

            // set 0: UBO caméra
            self.context.device.cmd_bind_descriptor_sets(
                frame.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.descriptor_handler.world_descriptor_sets[self.frame_index]],
                &[],
            );

            // set 2 : shadow map (unique, rebindée chaque frame). Le set 1 (texture)
            // reste bindé par objet dans la boucle de draw.
            self.context.device.cmd_bind_descriptor_sets(
                frame.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                2,
                &[self.descriptor_handler.shadow_descriptor_set],
                &[],
            );

            for render_object in &self.world.render_objects {
                // set 1 : la texture de CET objet
                self.context.device.cmd_bind_descriptor_sets(
                    frame.command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline_layout,
                    1, // first_set = 1
                    &[render_object.texture.descriptor_set],
                    &[],
                );
                // Matrice model (placement de l'objet dans le monde).
                let model = render_object.transform.to_matrix();
                // Matrice normale = inverse-transposée de la partie 3x3 du model.
                // Gère correctement rotation ET scale non-uniforme. On la stocke
                // dans une Mat4 (from_mat3 complète avec une 4e ligne/col identité)
                // pour respecter l'alignement 16 octets du push constant.
                let normal_matrix =
                    glam::Mat4::from_mat3(glam::Mat3::from_mat4(model).inverse().transpose());
                // offset 0 : model
                self.context.device.cmd_push_constants(
                    frame.command_buffer,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
                    0,
                    bytemuck::bytes_of(&model),
                );
                // offset 64 : matrice normale (juste après les 64 octets de model)
                self.context.device.cmd_push_constants(
                    frame.command_buffer,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
                    std::mem::size_of::<glam::Mat4>() as u32,
                    bytemuck::bytes_of(&normal_matrix),
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
        }

        self.context.transition_image_layout(
            frame.command_buffer,
            &[(
                self.swapchain.color_images[image_index as usize],
                ImageLayoutState::RENDERABLE_COLOR_IMAGE_STATE,
                ImageLayoutState::PRESENT_COLOR_IMAGE_STATE,
            )],
            1,
        );
    }
}
