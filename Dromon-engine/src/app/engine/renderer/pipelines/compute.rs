use anyhow::Result;
use ash::vk;
use std::ffi::CStr;

/// Pipeline compute : un shader et son layout, rien d'autre. Pas d'état fixe
/// (rasterisation, profondeur, blending…) : le shader lit et écrit directement les
/// ressources déclarées dans `pipeline_layout` (descriptor sets, push constants).
/// Le module peut être détruit dès la pipeline créée.
pub(crate) fn create_compute_pipeline(
    device: &ash::Device,
    shader: vk::ShaderModule,
    pipeline_layout: vk::PipelineLayout,
    pipeline_cache: vk::PipelineCache,
) -> Result<vk::Pipeline> {
    // slangc renomme le point d'entrée en `main` dans le SPIR-V.
    let entry_point = CStr::from_bytes_with_nul(b"main\0")?;
    unsafe {
        Ok(device
            .create_compute_pipelines(
                pipeline_cache,
                &[vk::ComputePipelineCreateInfo::default()
                    .stage(
                        vk::PipelineShaderStageCreateInfo::default()
                            .stage(vk::ShaderStageFlags::COMPUTE)
                            .module(shader)
                            .name(entry_point),
                    )
                    .layout(pipeline_layout)],
                None,
            )
            .map_err(|(_, e)| e)?[0])
    }
}
