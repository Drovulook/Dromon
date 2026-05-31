mod engine;

use winit::application::ApplicationHandler;

use crate::app::engine::Engine;

pub struct App {
    engine: Option<Engine>,
    use_cli: bool,
}

impl App {
    pub fn new(use_cli: bool) -> Self {
        Self { engine: None, use_cli }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.engine = Some(Engine::new(event_loop, self.use_cli).unwrap());
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
