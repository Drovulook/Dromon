use anyhow::Result;
use ash::vk;
use std::ffi::CStr;

/// Pipeline du ciel : un triangle plein écran posé au plan far (z = 1),
/// dessiné APRÈS la géométrie opaque. Différences avec la pipeline principale :
/// - aucun vertex input : le vertex shader génère ses 3 sommets via `SV_VertexID`
///   (draw de 3 sommets, sans vertex buffer).
/// - pas de culling : on ne dépend pas du sens de rotation du triangle.
/// - profondeur en lecture seule, test `LESS_OR_EQUAL` : ne passe que là où le
///   depth buffer vaut encore la valeur de clear (1.0), i.e. où rien n'est dessiné.
pub(crate) fn create_sky_pipeline(
    device: &ash::Device,
    vertex_shader: vk::ShaderModule,
    fragment_shader: vk::ShaderModule,
    pipeline_layout: vk::PipelineLayout,
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
                    // vide : sommets générés dans le shader.
                    .vertex_input_state(&vk::PipelineVertexInputStateCreateInfo::default())
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
                    // doit égaler celui des autres pipelines de la passe.
                    .multisample_state(
                        &vk::PipelineMultisampleStateCreateInfo::default()
                            .rasterization_samples(msaa_samples),
                    )
                    .depth_stencil_state(
                        &vk::PipelineDepthStencilStateCreateInfo::default()
                            .depth_test_enable(true)
                            .depth_write_enable(false)
                            // `LESS` échouerait partout : 1.0 < 1.0 est faux.
                            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL)
                            .depth_bounds_test_enable(false)
                            .stencil_test_enable(false),
                    )
                    .color_blend_state(
                        &vk::PipelineColorBlendStateCreateInfo::default()
                            .logic_op_enable(false)
                            .attachments(&[vk::PipelineColorBlendAttachmentState::default()
                                .color_write_mask(vk::ColorComponentFlags::RGBA)
                                .blend_enable(false)]),
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
