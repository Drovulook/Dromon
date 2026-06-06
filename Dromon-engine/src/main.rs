use crate::app::{App, logger::Logger};
use anyhow::Result;
use std::sync::Arc;
use winit::event_loop::{ControlFlow, EventLoop};

mod app;

fn main() -> Result<()> {
    let use_cli = std::env::args().any(|a| a == "--use-cli");
    let logger = Arc::new(Logger::new(use_cli));

    let event_loop = EventLoop::new().map_err(|e| {
        logger.error(&format!("Impossible de créer l'EventLoop : {e}"));
        e
    })?;

    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new(logger.clone());
    let loop_result = event_loop.run_app(&mut app);

    let pending = app.pending_error.take();

    if let Err(ref e) = loop_result {
        logger.error(&format!("Erreur de boucle d'événements : {e:#}"));
    }

    let failed = pending.is_some() || loop_result.is_err();

    if use_cli && failed {
        drop(app);
        drop(logger);
        std::process::exit(1);
    }

    if let Some(e) = pending {
        return Err(e);
    }
    loop_result?;
    Ok(())
}
