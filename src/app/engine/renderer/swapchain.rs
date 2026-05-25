use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;
use winit::window::Window;

pub struct Swapchain {
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    image_views: Vec<vk::ImageView>,
    images: Vec<vk::Image>,
    handle: vk::SwapchainKHR,
    surface: vk::SurfaceKHR,
    window: Arc<Window>,
}

impl Swapchain {
    pub fn new(context: Arc<RenderingContext>, window: Arc<Window>) -> Result<Self> {
        let surface = unsafe { context.create_surface(window.clone())? };
        let surface_capabilities = unsafe { context.get_surface_capabilities(surface)? };
        let surface_formats = unsafe { context.get_surface_formats(surface)? };
        Ok(Self {})
    }
}
