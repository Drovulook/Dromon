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

            //////////////////////////// Chaque système bind sa propre shadow pipeline /////////////////////////////
            self.object_render_system.record_shadow_objects(
                command_buffer,
                frame_index,
                &self.world.render_objects,
            );

            self.terrain_render_sytem.record_shadow_terrain(
                command_buffer,
                frame_index,
                &self.world.terrain_meshes,
            );

            /////////////////////////////////////////////////////////////////////////////////////////////////////////

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

            //////////////////// calls of record record_render_... functions from render sustems ////////////////////

            self.object_render_system.record_render_objects(
                frame.command_buffer,
                self.frame_index,
                &self.world.render_objects,
            );

            self.terrain_render_sytem.record_render_terrain(
                frame.command_buffer,
                self.frame_index,
                &self.world.terrain_meshes,
            );

            /////////////////////////////////////////////////////////////////////////////////////////////////////////

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
