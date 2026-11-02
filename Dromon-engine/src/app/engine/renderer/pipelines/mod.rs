//! Création des pipelines (graphiques, compute) et chargement des shaders : un fichier
//! par pipeline.

mod compute;
mod graphics;
mod shadow;
mod sky;
mod water;

use anyhow::Result;
use ash::vk;
use std::io;

pub(crate) use compute::create_compute_pipeline;
pub(crate) use graphics::create_graphics_pipeline;
pub(crate) use shadow::create_shadow_pipeline;
pub(crate) use sky::create_sky_pipeline;
pub(crate) use water::create_water_pipeline;

const SHADERS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/res/shaders/");

pub(crate) fn load_shader_module(device: &ash::Device, path: &str) -> Result<vk::ShaderModule> {
    let code = std::fs::read(format!("{}{}", SHADERS_DIR, path))?;
    let code = ash::util::read_spv(&mut io::Cursor::new(code))?;
    create_shader_module(device, &code)
}

/// Module à partir de SPIR-V déjà en mémoire (mots de 32 bits).
pub(crate) fn create_shader_module(device: &ash::Device, code: &[u32]) -> Result<vk::ShaderModule> {
    let create_info = vk::ShaderModuleCreateInfo::default().code(code);
    Ok(unsafe { device.create_shader_module(&create_info, None)? })
}
