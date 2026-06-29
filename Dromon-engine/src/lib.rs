mod app;
mod scene;

use crate::app::{App, logger::Logger};
use anyhow::Result;
use std::sync::Arc;
use winit::event_loop::{ControlFlow, EventLoop};

// ─── Façade publique du moteur ────────────────────────────────────────────────
// Les modules internes restent `pub(crate)` ; on ne ré-expose que l'API utile aux
// applications. C'est le patron « facade » : l'extérieur voit `dromon_engine::World`
// sans rien savoir de l'arborescence `app::engine::renderer::…`.
pub use app::engine::renderer::render_resources::{RenderObject, Transform};
pub use app::engine::renderer::world::World;
pub use app::engine::terrain_generation::{GenParams, HeightParams};
pub use app::engine::timer::Timer;
pub use scene::Scene;

/// Point d'entrée du moteur : construit la fenêtre/contexte Vulkan et lance la
/// boucle d'événements avec la `Scene` fournie par l'application.
pub fn run<S: Scene + 'static>(scene: S) -> Result<()> {
    let use_cli = std::env::args().any(|a| a == "--use-cli");
    let logger = Arc::new(Logger::new(use_cli));

    let event_loop = EventLoop::new().map_err(|e| {
        logger.error(&format!("Impossible de créer l'EventLoop : {e}"));
        e
    })?;

    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new(logger.clone(), Box::new(scene));
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
