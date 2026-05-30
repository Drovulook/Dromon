mod engine;

use winit::{application::ApplicationHandler, window::WindowAttributes};

use crate::app::engine::Engine;

#[derive(Default)]
pub struct App {
    engine: Option<Engine>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.engine = Some(Engine::new(event_loop).unwrap()); //TODO: gérer les erreurs proprement
        // if let Some(engine) = self.engine.as_mut() {
        //     let secondary_window_attributes =
        //         WindowAttributes::default().with_title("Secondary window");
        //     let secondary_window = engine.create_window(event_loop, secondary_window_attributes);
        // }
    }

    fn suspended(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.engine = None;
    }

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        //request redraw
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
