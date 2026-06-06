mod engine;
pub mod logger;
mod socket_client;

use std::sync::Arc;

use anyhow::Error;
use logger::Logger;
use winit::application::ApplicationHandler;

use crate::app::engine::Engine;

pub struct App {
    engine: Option<Engine>,
    logger: Arc<Logger>,
    pub pending_error: Option<Error>,
}

impl App {
    pub fn new(logger: Arc<Logger>) -> Self {
        Self { engine: None, logger, pending_error: None }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        match Engine::new(event_loop, self.logger.clone()) {
            Ok(engine) => self.engine = Some(engine),
            Err(e) => {
                self.logger.error(&format!("Impossible d'initialiser le moteur : {e:#}"));
                self.pending_error = Some(e);
                event_loop.exit();
            }
        }
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
            if let Err(e) = engine.window_event(event_loop, window_id, event) {
                self.logger.error(&format!("Erreur fatale dans la boucle d'événements : {e:#}"));
                self.pending_error = Some(e);
                event_loop.exit();
            }
        }
    }
}
