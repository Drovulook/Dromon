mod context;

use anyhow::Context as AnyhowContext;
use anyhow::{Result, anyhow};
use ash::vk;
use context::Context as VulkanContext;
use softbuffer::{Context, Surface};
use std::{num::NonZeroU32, sync::Arc};
use winit::window::Window;

use crate::app::engine::renderer::context::{ContextAttributes, queue_family_picker};

pub struct Renderer {
    surface: Surface<Arc<Window>, Arc<Window>>,
    vk_context: VulkanContext,
}

impl Renderer {
    pub(crate) fn new(window: Arc<Window>) -> Result<Self> {
        let context = Context::new(window.clone()).map_err(|e| anyhow!("{e}"))?;
        let surface = Surface::new(&context, window.clone()).map_err(|e| anyhow!("{e}"))?;

        let vk_context = VulkanContext::new(ContextAttributes {
            window,
            queue_family_picker: queue_family_picker::single_queue_family,
        })?;

        Ok(Self {
            surface,
            vk_context,
        })
    }

    pub(crate) fn draw(&mut self) -> Result<()> {
        let (width, height) = {
            let size = self.surface.window().inner_size();
            (size.width, size.height)
        };
        self.surface
            .resize(
                NonZeroU32::new(width).unwrap_or(NonZeroU32::new(1).unwrap()),
                NonZeroU32::new(height).unwrap_or(NonZeroU32::new(1).unwrap()),
            )
            .map_err(|e| anyhow!("{e}"))?;
        let mut buffer = self.surface.buffer_mut().map_err(|e| anyhow!("{e}"))?;
        buffer.fill(0x00000000);
        buffer.present().map_err(|e| anyhow!("{e}"))?;
        Ok(())
    }
}
