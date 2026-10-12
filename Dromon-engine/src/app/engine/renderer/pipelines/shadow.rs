use anyhow::Result;
use ash::vk;
use std::ffi::CStr;

/// Pipeline « depth-only » de la passe d'ombre. Différences clés avec la
/// pipeline principale :
/// - un seul shader (vertex) : on ne produit AUCUNE couleur, juste la
///   profondeur. `PipelineRenderingCreateInfo` n'a donc aucun color attachment.
/// - pas de MSAA (1 sample) : la shadow map est une simple image de profondeur.
/// - depth bias activé : décale légèrement la profondeur écrite pour éviter le
///   « shadow acne » (auto-ombrage dû à la précision finie de la carte). Le
///   facteur « slope » augmente le biais sur les surfaces rasantes, là où
///   l'acne est le pire.
pub(crate) fn create_shadow_pipeline(
    device: &ash::Device,
    vertex_shader: vk::ShaderModule,
    pipeline_layout: vk::PipelineLayout,
    vertex_bindings: &[vk::VertexInputBindingDescription],
    vertex_attributes: &[vk::VertexInputAttributeDescription],
    depth_format: vk::Format,
    pipeline_cache: vk::PipelineCache,
) -> Result<vk::Pipeline> {
    let entry_point = CStr::from_bytes_with_nul(b"main\0")?;
    unsafe {
        Ok(device
            .create_graphics_pipelines(
                pipeline_cache,
                &[vk::GraphicsPipelineCreateInfo::default()
                    // un seul stage : le vertex shader d'ombre.
                    .stages(&[vk::PipelineShaderStageCreateInfo::default()
                        .stage(vk::ShaderStageFlags::VERTEX)
                        .module(vertex_shader)
                        .name(entry_point)])
                    .vertex_input_state(
                        &vk::PipelineVertexInputStateCreateInfo::default()
                            .vertex_binding_descriptions(vertex_bindings)
                            .vertex_attribute_descriptions(vertex_attributes),
                    )
                    .input_assembly_state(
                        &vk::PipelineInputAssemblyStateCreateInfo::default()
                            .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
                            .primitive_restart_enable(false),
                    )
                    .viewport_state(
                        &vk::PipelineViewportStateCreateInfo::default()
                            .viewport_count(1)
                            .scissor_count(1),
                    )
                    .rasterization_state(
                        &vk::PipelineRasterizationStateCreateInfo::default()
                            .polygon_mode(vk::PolygonMode::FILL)
                            .cull_mode(vk::CullModeFlags::BACK)
                            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
                            .depth_clamp_enable(false)
                            .rasterizer_discard_enable(false)
                            // biais de profondeur statique (valeurs classiques).
                            .depth_bias_enable(true)
                            .depth_bias_constant_factor(1.25)
                            .depth_bias_slope_factor(1.75)
                            .line_width(1.0),
                    )
                    .multisample_state(
                        &vk::PipelineMultisampleStateCreateInfo::default()
                            .rasterization_samples(vk::SampleCountFlags::TYPE_1),
                    )
                    .depth_stencil_state(
                        &vk::PipelineDepthStencilStateCreateInfo::default()
                            .depth_test_enable(true)
                            .depth_write_enable(true)
                            .depth_compare_op(vk::CompareOp::LESS)
                            .depth_bounds_test_enable(false)
                            .stencil_test_enable(false),
                    )
                    // aucun color attachment → pas de color blend state utile.
                    .dynamic_state(
                        &vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&[
                            vk::DynamicState::VIEWPORT,
                            vk::DynamicState::SCISSOR,
                        ]),
                    )
                    .layout(pipeline_layout)
                    .push_next(
                        &mut vk::PipelineRenderingCreateInfo::default()
                            .depth_attachment_format(depth_format),
                    )],
                None,
            )
            .map_err(|(_, e)| e)?[0])
    }
}
