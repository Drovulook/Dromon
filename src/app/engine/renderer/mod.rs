mod swapchain;

use crate::app::engine::rendering_context::RenderingContext;
use anyhow::{Result, anyhow};
use softbuffer::Context as SoftBufferContext;
use softbuffer::Surface;
use std::{num::NonZeroU32, sync::Arc};
use winit::window::Window;

pub struct Renderer {
    surface: Surface<Arc<Window>, Arc<Window>>,
    swapchain: swapchain::Swapchain,
    context: Arc<RenderingContext>,
}

impl Renderer {
    pub(crate) fn new(context: Arc<RenderingContext>, window: Arc<Window>) -> Result<Self> {
        let softbuffer_context =
            SoftBufferContext::new(window.clone()).map_err(|e| anyhow!("{e}"))?;
        let surface =
            Surface::new(&softbuffer_context, window.clone()).map_err(|e| anyhow!("{e}"))?;
        let mut swapchain = swapchain::Swapchain::new(context.clone(), window.clone())?;
        swapchain.update_size()?;

        Ok(Self {
            surface,
            context,
            swapchain,
        })
    }

    pub fn resize(&mut self) -> Result<()> {
        self.swapchain.update_size()
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
