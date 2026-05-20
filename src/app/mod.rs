mod engine;

use winit::application::ApplicationHandler;

use crate::app::engine::Engine;

#[derive(Default)]
pub struct App {
    engine: Option<Engine>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.engine = Some(Engine::new(event_loop).unwrap()); //TODO: gérer les erreurs proprement
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        match event {
            //HACK : workaround : display a window by filling the buffer using softbuffer for Wayland
            winit::event::WindowEvent::CloseRequested => event_loop.exit(),
            winit::event::WindowEvent::RedrawRequested => {
                if let Some(engine) = &mut self.engine {
                    engine.draw(window_id);
                }
            }
            _ => {}
        }
    }
}
