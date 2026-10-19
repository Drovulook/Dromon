use anyhow::Result;
use ash::vk;
use glam::IVec2;
use std::sync::Arc;

use crate::{
    app::engine::{
        renderer::{
            descriptors::DescriptorHandler,
            pipelines,
            render_resources::{MaterialBuffer, TerrainVertex, WorldMaps},
            world::terrain::meshes::InstalledChunks,
        },
        rendering_context::RenderingContext,
    },
    profile,
};
use glam::Mat4;

pub(crate) struct TerrainRenderSystem {
    context: Arc<RenderingContext>,
    descriptor_handler: Arc<DescriptorHandler>,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    // Pipeline d'ombre du terrain : même `shadow_vert.spv` (position seule), mais
    // fige le format `TerrainVertex` — donc distincte de celle des objets.
    shadow_pipeline: vk::Pipeline,
}

impl TerrainRenderSystem {
    pub(crate) fn new(
        context: Arc<RenderingContext>,
        descriptor_handler: Arc<DescriptorHandler>,
        color_format: vk::Format,
        depth_format: vk::Format,
        msaa_samples: vk::SampleCountFlags,
        extent: vk::Extent2D,
        shadow_map_format: vk::Format,
        polygon_mode: vk::PolygonMode,
    ) -> Result<Self> {
        unsafe {
            let vertex_shader = pipelines::load_shader_module(&context.device,"terrain_vert.spv")?;
            let fragment_shader = pipelines::load_shader_module(&context.device,"terrain_frag.spv")?;
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
                        descriptor_handler.texture_descriptor_set_layout,
                        // set 2 : shadow map (unique, partagée par toutes les frames).
                        descriptor_handler.shadow_descriptor_set_layout,
                        // set 3 : cartes du monde (masque de la mer).
                        descriptor_handler.world_maps_descriptor_set_layout,
                        // set 4 : table des matériaux (SSBO).
                        descriptor_handler.materials_descriptor_set_layout,
                    ])
                    .push_constant_ranges(&push_constant_ranges),
                None,
            )?;

            let pipeline = pipelines::create_graphics_pipeline(
                &context.device,
                vertex_shader,
                fragment_shader,
                pipeline_layout,
                &[TerrainVertex::get_binding_description()],
                &TerrainVertex::get_attribute_descriptions(),
                extent,
                color_format,
                depth_format,
                msaa_samples,
                polygon_mode,
                vk::PipelineCache::default(),
            )?;

            context.device.destroy_shader_module(vertex_shader, None);
            context.device.destroy_shader_module(fragment_shader, None);

            // Pipeline d'ombre du terrain : réutilise shadow_vert.spv (ne lit que la
            // position, donc compatible avec TerrainVertex) + le pipeline_layout.
            let shadow_vertex_shader = pipelines::load_shader_module(&context.device,"terrain_shadow_vert.spv")?;
            let shadow_pipeline = pipelines::create_shadow_pipeline(
                &context.device,
                shadow_vertex_shader,
                pipeline_layout,
                &[TerrainVertex::get_binding_description()],
                &TerrainVertex::get_attribute_descriptions(),
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

    pub(crate) fn record_render_terrain(
        &self,
        command_buffer: vk::CommandBuffer,
        frame_index: usize,
        chunks: &InstalledChunks,
        visible_chunks: &[IVec2],
        world_maps: &WorldMaps,
        materials: &MaterialBuffer,
    ) {
        if chunks.is_empty() {
            return;
        }

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

            // set 2 : shadow map (unique, rebindée chaque frame). Le terrain n'a
            // pas de set 1 (texture) : son frag shader n'échantillonne aucune
            // texture de matériau pour l'instant.
            self.context.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                2,
                &[self.descriptor_handler.shadow_descriptor_set],
                &[],
            );

            // set 3 : cartes du monde
            self.context.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                3,
                &[world_maps.descriptor_set],
                &[],
            );

            // set 4 : table des matériaux
            self.context.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                4,
                &[materials.descriptor_set],
                &[],
            );

            // Les sommets du terrain sont déjà en coordonnées monde → `model` =
            // identité. La matrice normale n'est pas utilisée (le frag shader
            // recalcule la normale depuis les dérivées d'écran), mais le push
            // constant range en attend une : on pousse l'identité.
            let identity = Mat4::IDENTITY;
            self.context.device.cmd_push_constants(
                command_buffer,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                bytemuck::bytes_of(&identity),
            );
            self.context.device.cmd_push_constants(
                command_buffer,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                std::mem::size_of::<Mat4>() as u32,
                bytemuck::bytes_of(&identity),
            );

            for coord in visible_chunks {
                {
                    let Some(mesh) = chunks.get(coord).map(|c| &c.mesh) else {
                        continue;
                    };
                    profile!("bind + draw command - terrain mesh");
                    mesh.bind(command_buffer);
                    self.context.device.cmd_draw_indexed(
                        command_buffer,
                        mesh.index_count(),
                        1,
                        0,
                        0,
                        0,
                    );
                }
            }
        }
    }

    pub(crate) fn record_shadow_terrain(
        &self,
        command_buffer: vk::CommandBuffer,
        frame_index: usize,
        chunks: &InstalledChunks,
        shadow_chunks: &[IVec2],
    ) {
        if chunks.is_empty() {
            return;
        }

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

            // model = identité (sommets déjà en coordonnées monde).
            let identity = Mat4::IDENTITY;
            self.context.device.cmd_push_constants(
                command_buffer,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                bytemuck::bytes_of(&identity),
            );

            for coord in shadow_chunks {
                let Some(mesh) = chunks.get(coord).map(|c| &c.mesh) else {
                    continue;
                };
                mesh.bind(command_buffer);
                self.context.device.cmd_draw_indexed(
                    command_buffer,
                    mesh.index_count(),
                    1,
                    0,
                    0,
                    0,
                );
            }
        }
    }
}

impl Drop for TerrainRenderSystem {
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
