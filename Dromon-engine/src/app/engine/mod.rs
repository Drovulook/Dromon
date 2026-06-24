pub mod renderer;
pub mod timer;

mod debug_messenger;
mod rendering_context;

use crate::app::engine::rendering_context::{
    ContextAttributes, RenderingContext, queue_family_picker,
};
use crate::app::engine::timer::Timer;
use crate::app::logger::Logger;
use anyhow::Result;
use renderer::Renderer;
use std::{collections::HashMap, sync::Arc};
use winit::{
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowId},
};

#[cfg(target_os = "linux")]
use winit::platform::wayland::WindowAttributesExtWayland;

pub struct Engine {
    renderers: HashMap<WindowId, Renderer>,
    windows: HashMap<WindowId, Arc<Window>>,
    primary_window_id: WindowId,
    rendering_context: Arc<RenderingContext>,
    logger: Arc<Logger>,
    timer: Timer,
}

impl Engine {
    pub fn new(event_loop: &ActiveEventLoop, logger: Arc<Logger>) -> Result<Self> {
        let attrs = WindowAttributes::default().with_title("Dromon");
        #[cfg(target_os = "linux")]
        let attrs = attrs.with_name("dromon", "dromon");
        let primary_window = Arc::new(event_loop.create_window(attrs)?);

        let primary_window_id = primary_window.id();

        let rendering_context = Arc::new(RenderingContext::new(
            logger.clone(),
            ContextAttributes {
                compatibility_window: &primary_window,
                queue_family_picker: queue_family_picker::single_queue_family,
            },
        )?);

        let windows = HashMap::from([(primary_window_id, primary_window.clone())]);

        let renderers = windows
            .iter()
            .map(|(id, window)| {
                let renderer =
                    Renderer::new(rendering_context.clone(), window.clone(), logger.clone())?;
                Ok((*id, renderer))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        logger.state("Running");

        Ok(Self {
            renderers,
            windows,
            primary_window_id,
            rendering_context,
            logger,
            timer: Timer::new(),
        })
    }

    pub fn request_redraw(&self) {
        for window in self.windows.values() {
            window.request_redraw();
        }
    }

    pub(crate) fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: winit::event::WindowEvent,
    ) -> Result<()> {
        match event {
            winit::event::WindowEvent::CloseRequested => {
                if window_id == self.primary_window_id {
                    event_loop.exit();
                } else {
                    self.windows.remove(&window_id);
                    self.renderers.remove(&window_id);
                }
            }

            winit::event::WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderers.get_mut(&window_id) {
                    renderer.render(&self.timer)?;
                }
                self.timer.tick();
                self.logger.fps(self.timer.fps());
            }

            winit::event::WindowEvent::Resized(_size) => {
                if let Some(renderer) = self.renderers.get_mut(&window_id) {
                    renderer.resize();
                }
            }

            winit::event::WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(renderer) = self.renderers.get_mut(&window_id) {
                    renderer.resize();
                }
            }

            _ => {}
        }
        Ok(())
    }

    pub fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
        logger: Arc<Logger>,
    ) -> Result<WindowId> {
        let window = Arc::new(event_loop.create_window(attributes)?);
        let window_id = window.id();
        self.windows.insert(window_id, window.clone());
        self.renderers.insert(
            window_id,
            Renderer::new(self.rendering_context.clone(), window.clone(), logger)?,
        );
        Ok(window_id)
    }
}
