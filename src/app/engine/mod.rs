mod renderer;
mod rendering_context;

use crate::app::engine::rendering_context::{
    ContextAttributes, RenderingContext, queue_family_picker,
};
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
}

impl Engine {
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self> {
        let attrs = WindowAttributes::default().with_title("Dromon");
        #[cfg(target_os = "linux")]
        let attrs = attrs.with_name("dromon", "dromon");
        let primary_window = Arc::new(event_loop.create_window(attrs)?);

        let primary_window_id = primary_window.id();
        let windows = HashMap::from([(primary_window_id, primary_window.clone())]);

        let rendering_context = Arc::new(RenderingContext::new(ContextAttributes {
            compatibility_window: &primary_window,
            queue_family_picker: queue_family_picker::single_queue_family,
        })?);

        let renderers = windows
            .iter()
            .map(|(id, window)| {
                let renderer = Renderer::new(rendering_context.clone(), window.clone()).unwrap();
                (*id, renderer)
            })
            .collect::<HashMap<_, _>>();

        Ok(Self {
            renderers,
            windows,
            primary_window_id,
            rendering_context,
        })
    }
    pub fn draw(&mut self, window_id: WindowId) {
        if let Some(renderer) = self.renderers.get_mut(&window_id) {
            renderer.draw().unwrap();
        }
        if let Some(window) = self.windows.get(&window_id) {
            window.request_redraw();
        }
    }

    pub(crate) fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            //HACK : workaround : display a window by filling the buffer using softbuffer for Wayland
            winit::event::WindowEvent::CloseRequested => {
                if window_id == self.primary_window_id {
                    event_loop.exit();
                } else {
                    self.windows.remove(&window_id);
                    self.renderers.remove(&window_id);
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                self.draw(window_id);
            }
            _ => {}
        }
    }

    pub fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
    ) -> Result<WindowId> {
        let window = Arc::new(event_loop.create_window(attributes)?);
        let window_id = window.id();
        self.windows.insert(window_id, window.clone());
        self.renderers.insert(
            window_id,
            Renderer::new(self.rendering_context.clone(), window.clone())?,
        );
        Ok(window_id)
    }
}
