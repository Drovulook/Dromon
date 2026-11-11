use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use crate::app::engine::{
    renderer::{
        descriptors::DescriptorHandler,
        pipelines,
        render_resources::{MaterialBuffer, TerrainVertex, WorldMaps},
        world::terrain::{
            arenas::TerrainArenas,
            draw_buffers::{DrawBuffers, IndirectDraws},
            meshes::ChunkSeam,
        },
    },
    rendering_context::RenderingContext,
};

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
            let vertex_shader = pipelines::load_shader_module(&context.device, "terrain_vert.spv")?;
            let fragment_shader =
                pipelines::load_shader_module(&context.device, "terrain_frag.spv")?;
            let pipeline_layout = context.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&[
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
                ]),
                None,
            )?;

            // Binding 0 : sommets (par sommet) ; binding 1 : couture du chunk (par
            // instance, désignée par le `first_instance` de chaque commande).
            let bindings = [
                TerrainVertex::get_binding_description(),
                ChunkSeam::binding_description(),
            ];
            let attributes: Vec<_> = TerrainVertex::get_attribute_descriptions()
                .into_iter()
                .chain(ChunkSeam::attribute_descriptions())
                .collect();
            let pipeline = pipelines::create_graphics_pipeline(
                &context.device,
                vertex_shader,
                fragment_shader,
                pipeline_layout,
                &bindings,
                &attributes,
                extent,
                color_format,
                depth_format,
                msaa_samples,
                polygon_mode,
                vk::PipelineCache::default(),
            )?;

            context.device.destroy_shader_module(vertex_shader, None);
            context.device.destroy_shader_module(fragment_shader, None);

            // Pipeline d'ombre du terrain : position seule, sans couture (binding 0 seul),
            // même pipeline_layout.
            let shadow_vertex_shader =
                pipelines::load_shader_module(&context.device, "terrain_shadow_vert.spv")?;
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
        arenas: &TerrainArenas,
        draws: &DrawBuffers,
        world_maps: &WorldMaps,
        materials: &MaterialBuffer,
    ) {
        let commands = draws.camera(frame_index);
        if commands.count == 0 {
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

            self.bind_arenas(command_buffer, arenas);
            self.context.device.cmd_bind_vertex_buffers(
                command_buffer,
                ChunkSeam::BINDING,
                &[draws.seams(frame_index)],
                &[0],
            );
            self.draw(command_buffer, commands);
        }
    }

    pub(crate) fn record_shadow_terrain(
        &self,
        command_buffer: vk::CommandBuffer,
        frame_index: usize,
        arenas: &TerrainArenas,
        draws: &DrawBuffers,
    ) {
        // Corps seuls : les dalles ne projettent rien (cf. `terrainShadowVertMain`).
        let commands = draws.shadow(frame_index);
        if commands.count == 0 {
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

            self.bind_arenas(command_buffer, arenas);
            self.draw(command_buffer, commands);
        }
    }

    /// Un vertex buffer et un index buffer pour tous les chunks : chaque commande y
    /// désigne sa tranche par `first_index` et `vertex_offset`.
    fn bind_arenas(&self, command_buffer: vk::CommandBuffer, arenas: &TerrainArenas) {
        unsafe {
            self.context.device.cmd_bind_vertex_buffers2(
                command_buffer,
                0,
                &[arenas.vertices.buffer()],
                &[0],
                None,
                None,
            );
            self.context.device.cmd_bind_index_buffer(
                command_buffer,
                arenas.indices.buffer(),
                0,
                vk::IndexType::UINT32,
            );
        }
    }

    /// Toutes les commandes d'une passe en un appel : le GPU les lit dans le buffer, dans
    /// l'ordre (du proche au lointain pour la caméra).
    fn draw(&self, command_buffer: vk::CommandBuffer, commands: IndirectDraws) {
        unsafe {
            self.context.device.cmd_draw_indexed_indirect(
                command_buffer,
                commands.buffer,
                0,
                commands.count,
                IndirectDraws::STRIDE,
            );
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
