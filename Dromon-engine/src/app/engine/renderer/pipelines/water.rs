use anyhow::Result;
use ash::vk;
use std::ffi::CStr;

/// Pipeline de l'eau : surface semi-transparente dessinée EN DERNIER (après le
/// ciel). Différences avec la pipeline principale :
/// - blending **premultiplied alpha** : `couleur = src + dst · (1 − α)`, le shader
///   renvoyant `src = F · reflet` et `α = F` (Fresnel). La sortie d'un attachment
///   UNORM est bornée à 1 AVANT le blending : ici le reflet du soleil (≫ 1) est
///   multiplié par `F` avant la borne, et non après comme en alpha classique.
/// - profondeur testée mais **pas écrite** : l'eau est cachée par le terrain
///   émergé, sans masquer ce qui serait dessiné après elle.
/// - pas de culling : surface visible aussi par en dessous.
/// - derrière l'eau, toujours un fond : le terrain, ou au-delà du terrain chargé
///   l'eau profonde peinte par le ciel (sky.slang).
pub(crate) fn create_water_pipeline(
    device: &ash::Device,
    vertex_shader: vk::ShaderModule,
    fragment_shader: vk::ShaderModule,
    pipeline_layout: vk::PipelineLayout,
    vertex_bindings: &[vk::VertexInputBindingDescription],
    vertex_attributes: &[vk::VertexInputAttributeDescription],
    color_format: vk::Format,
    depth_format: vk::Format,
    msaa_samples: vk::SampleCountFlags,
    pipeline_cache: vk::PipelineCache,
) -> Result<vk::Pipeline> {
    let entry_point = CStr::from_bytes_with_nul(b"main\0")?;
    unsafe {
        Ok(device
            .create_graphics_pipelines(
                pipeline_cache,
                &[vk::GraphicsPipelineCreateInfo::default()
                    .stages(&[
                        vk::PipelineShaderStageCreateInfo::default()
                            .stage(vk::ShaderStageFlags::VERTEX)
                            .module(vertex_shader)
                            .name(entry_point),
                        vk::PipelineShaderStageCreateInfo::default()
                            .stage(vk::ShaderStageFlags::FRAGMENT)
                            .module(fragment_shader)
                            .name(entry_point),
                    ])
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
                            .cull_mode(vk::CullModeFlags::NONE)
                            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
                            .depth_clamp_enable(false)
                            .rasterizer_discard_enable(false)
                            .depth_bias_enable(false)
                            .line_width(1.0),
                    )
                    .multisample_state(
                        &vk::PipelineMultisampleStateCreateInfo::default()
                            .rasterization_samples(msaa_samples),
                    )
                    .depth_stencil_state(
                        &vk::PipelineDepthStencilStateCreateInfo::default()
                            .depth_test_enable(true)
                            .depth_write_enable(false)
                            // Reversed-Z : le plus proche a la plus GRANDE profondeur.
                            .depth_compare_op(vk::CompareOp::GREATER)
                            .depth_bounds_test_enable(false)
                            .stencil_test_enable(false),
                    )
                    .color_blend_state(
                        &vk::PipelineColorBlendStateCreateInfo::default()
                            .logic_op_enable(false)
                            .attachments(&[vk::PipelineColorBlendAttachmentState::default()
                                .color_write_mask(vk::ColorComponentFlags::RGBA)
                                .blend_enable(true)
                                .src_color_blend_factor(vk::BlendFactor::ONE)
                                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                                .color_blend_op(vk::BlendOp::ADD)
                                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                                .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
                                .alpha_blend_op(vk::BlendOp::ADD)]),
                    )
                    .dynamic_state(
                        &vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&[
                            vk::DynamicState::VIEWPORT,
                            vk::DynamicState::SCISSOR,
                        ]),
                    )
                    .layout(pipeline_layout)
                    .push_next(
                        &mut vk::PipelineRenderingCreateInfo::default()
                            .color_attachment_formats(&[color_format])
                            .depth_attachment_format(depth_format),
                    )],
                None,
            )
            .map_err(|(_, e)| e)?[0])
    }
}
