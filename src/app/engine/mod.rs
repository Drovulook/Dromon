mod renderer;

use anyhow::Result;
use renderer::Renderer;
use std::{collections::HashMap, sync::Arc};
use winit::{
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowId},
};

pub struct Engine {
    renderers: HashMap<WindowId, Renderer>,
    windows: HashMap<WindowId, Arc<Window>>,
    primary_window_id: WindowId,
}

impl Engine {
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self> {
        let primary_window = Arc::new(event_loop.create_window(WindowAttributes::default())?);
        let primary_window_id = primary_window.id();
        let windows = HashMap::from([(primary_window_id, primary_window.clone())]);
        let renderers = windows
            .iter()
            .map(|(id, window)| {
                let renderer = Renderer::new(window.clone()).unwrap();
                (*id, renderer)
            })
            .collect::<HashMap<_, _>>();

        Ok(Self {
            renderers,
            windows,
            primary_window_id,
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
}
