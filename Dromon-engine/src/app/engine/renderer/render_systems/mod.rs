mod object_render_system;
mod terrain_render_system;

use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
pub(crate) use object_render_system::ObjectRenderSystem;
pub(crate) use terrain_render_system::TerrainRenderSystem;
