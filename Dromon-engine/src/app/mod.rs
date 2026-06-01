mod engine;
pub mod logger;
mod socket_client;

use std::sync::Arc;

use logger::Logger;
use winit::application::ApplicationHandler;

use crate::app::engine::Engine;

pub struct App {
    engine: Option<Engine>,
    logger: Arc<Logger>,
}

impl App {
    pub fn new(logger: Arc<Logger>) -> Self {
        Self { engine: None, logger }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.engine = Some(Engine::new(event_loop, self.logger.clone()).unwrap());
    }

    fn suspended(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        self.engine = None;
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(engine) = &mut self.engine {
            engine.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if let Some(engine) = &mut self.engine {
            engine.window_event(event_loop, window_id, event);
        }
    }
}
