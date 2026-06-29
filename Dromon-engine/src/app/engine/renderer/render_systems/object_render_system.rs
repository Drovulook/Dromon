use crate::app::engine::renderer::descriptors::DescriptorHandler;
use crate::app::engine::renderer::render_resources::{ObjectVertex, RenderObject};
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

/// Wrapper autour des pipelines propres aux `RenderObject` (objets issus de
/// modèles 3D).
pub(crate) struct ObjectRenderSystem {
    context: Arc<RenderingContext>,
    descriptor_handler: Arc<DescriptorHandler>,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    // Pipeline depth-only de la passe d'ombre. Réutilise `pipeline_layout`
    // (set 0 + push constants suffisent au shadow vertex shader) et fige le
    // format `ObjectVertex` — d'où une pipeline d'ombre PAR système de rendu.
    shadow_pipeline: vk::Pipeline,
}

impl ObjectRenderSystem {
    pub(crate) fn new(
        context: Arc<RenderingContext>,
        descriptor_handler: Arc<DescriptorHandler>,
        color_format: vk::Format,
        depth_format: vk::Format,
        msaa_samples: vk::SampleCountFlags,
        extent: vk::Extent2D,
        shadow_map_format: vk::Format,
    ) -> Result<Self> {
        unsafe {
            let vertex_shader = context.load_shader_module("object_vert.spv")?;
            let fragment_shader = context.load_shader_module("object_frag.spv")?;
            // 2 matrices poussées par objet. La 1re est la matrice
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
                        // set 2 : shadow map (unique, partagée par toutes les frames).
                        descriptor_handler.shadow_descriptor_set_layout,
                    ])
                    .push_constant_ranges(&push_constant_ranges),
                None,
            )?;

            let pipeline = context.create_graphics_pipeline(
                vertex_shader,
                fragment_shader,
                pipeline_layout,
                &[ObjectVertex::get_binding_description()],
                &ObjectVertex::get_attribute_descriptions(),
                extent,
                color_format,
                depth_format,
                msaa_samples,
                vk::PipelineCache::default(),
            )?;

            context.device.destroy_shader_module(vertex_shader, None);
            context.device.destroy_shader_module(fragment_shader, None);

            // Pipeline de la passe d'ombre : un seul stage (shadow_vert.spv), qui ne
            // lit que la position. Réutilise `pipeline_layout` (set 0 + push constants
            // suffisent) et le format de profondeur de la shadow map.
            let shadow_vertex_shader = context.load_shader_module("object_shadow_vert.spv")?;
            let shadow_pipeline = context.create_shadow_pipeline(
                shadow_vertex_shader,
                pipeline_layout,
                &[ObjectVertex::get_binding_description()],
                &ObjectVertex::get_attribute_descriptions(),
                shadow_map_format,
                vk::PipelineCache::default(),
            )?;
            context
                .device
                .destroy_shader_module(shadow_vertex_shader, None);

            Ok(Self {
                context,
                descriptor_handler,
                pipeline,
                pipeline_layout,
                shadow_pipeline,
            })
        }
    }

    /// Enregistre les draws des `RenderObject` dans la passe principale. Bind sa
    /// pipeline, le set 0 (caméra) et le set 2 (shadow map) communs, puis pour
    /// chaque objet le set 1 (sa texture) + les push constants.
    pub(crate) fn record_render_objects(
        &self,
        command_buffer: vk::CommandBuffer,
        frame_index: usize,
        render_objects: &[RenderObject],
    ) {
        unsafe {
            self.context.device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );

            // set 0: UBO caméra
            self.context.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.descriptor_handler.world_descriptor_sets[frame_index]],
                &[],
            );

            // set 2 : shadow map (unique, rebindée chaque frame).
            self.context.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                2,
                &[self.descriptor_handler.shadow_descriptor_set],
                &[],
            );

            for render_object in render_objects {
                // set 1 : la texture de CET objet
                self.context.device.cmd_bind_descriptor_sets(
                    command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline_layout,
                    1, // first_set = 1
                    &[render_object.texture.descriptor_set],
                    &[],
                );
                // Matrice model (placement de l'objet dans le monde).
                let model = render_object.transform.to_matrix();
                // Matrice normale = inverse-transposée de la partie 3x3 du model.
                let normal_matrix =
                    glam::Mat4::from_mat3(glam::Mat3::from_mat4(model).inverse().transpose());
                // offset 0 : model
                self.context.device.cmd_push_constants(
                    command_buffer,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
                    0,
                    bytemuck::bytes_of(&model),
                );
                // offset 64 : matrice normale (juste après les 64 octets de model)
                self.context.device.cmd_push_constants(
                    command_buffer,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
                    std::mem::size_of::<glam::Mat4>() as u32,
                    bytemuck::bytes_of(&normal_matrix),
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
        }
    }

    pub(crate) fn record_shadow_objects(
        &self,
        command_buffer: vk::CommandBuffer,
        frame_index: usize,
        render_objects: &[RenderObject],
    ) {
        unsafe {
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

            for render_object in render_objects {
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
        }
    }
}

impl Drop for ObjectRenderSystem {
    fn drop(&mut self) {
        unsafe {
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
