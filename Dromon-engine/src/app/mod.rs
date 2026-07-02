pub(crate) mod engine;
pub mod logger;
mod socket_client;

use std::sync::Arc;

use anyhow::Error;
use logger::Logger;
use winit::application::ApplicationHandler;

use crate::Scene;
use crate::app::engine::Engine;

pub struct App {
    engine: Option<Engine>,
    // La scène appartient à `App` (et non à `Engine`) pour rester stable à travers
    // les cycles suspend/resume, où l'`Engine` est détruit puis recréé. On passe
    // `&mut dyn Scene` aux méthodes du moteur au besoin.
    scene: Box<dyn Scene>,
    logger: Arc<Logger>,
    pub pending_error: Option<Error>,
}

impl App {
    pub fn new(logger: Arc<Logger>, scene: Box<dyn Scene>) -> Self {
        Self {
            engine: None,
            scene,
            logger,
            pending_error: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        match Engine::new(event_loop, self.logger.clone(), self.scene.as_mut()) {
            Ok(engine) => {
                self.engine = Some(engine);
                crate::profiling::initialize::flush(&self.logger);
            }
            Err(e) => {
                self.logger
                    .error(&format!("Impossible d'initialiser le moteur : {e:#}"));
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
            if let Err(e) = engine.window_event(event_loop, window_id, event, self.scene.as_mut()) {
                self.logger.error(&format!(
                    "Erreur fatale dans la boucle d'événements : {e:#}"
                ));
                self.pending_error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn device_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        if let Some(engine) = &mut self.engine {
            engine.device_event(event_loop, window_id, event);
        }
    }
}
