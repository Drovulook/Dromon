mod app;
mod app_dirs;
/// Fichiers de configuration RON (`dromon_engine::config::WorldGenConfig`…).
pub mod config;
pub mod profiling;
mod scene;

use crate::app::{App, logger::Logger};
use crate::config::EngineConfig;
use anyhow::Result;
use std::sync::Arc;
use winit::event_loop::{ControlFlow, EventLoop};

// ─── Façade publique du moteur ────────────────────────────────────────────────
// Les modules internes restent `pub(crate)` ; on ne ré-expose que l'API utile aux
// applications. C'est le patron « facade » : l'extérieur voit `dromon_engine::World`
// sans rien savoir de l'arborescence `app::engine::renderer::…`.
pub use app::engine::renderer::render_resources::{RenderObject, Transform};
pub use app::engine::renderer::world::World;
pub use app::engine::terrain_generation::{
    GridShape, MacroGrid, MacroMap, macro_map_cache, macro_map_export_dir,
};
pub use app_dirs::AppDirs;
/// Banc de mesure du maillage, pour `examples/mesh_bench.rs` uniquement.
#[doc(hidden)]
pub use app::engine::terrain_generation::bench::mesh_bench;
pub use app::engine::timer::Timer;
pub use scene::Scene;

/// Point d'entrée du moteur : construit la fenêtre/contexte Vulkan et lance la
/// boucle d'événements avec la `Scene` fournie par l'application. `dirs` : en pratique
/// [`app_dirs!`].
pub fn run<S: Scene + 'static>(scene: S, dirs: AppDirs) -> Result<()> {
    let use_cli = std::env::args().any(|a| a == "--use-cli");
    let use_profiling = std::env::args().any(|a| a == "--use-profiling");
    let (logger, requests) = Logger::new(&app_name(), use_cli);
    let logger = Arc::new(logger);
    if use_cli {
        Logger::install_panic_hook(&logger);
    }

    dirs.install().inspect_err(|e| logger.error(&format!("{e:#}")))?;

    // Arme la collecte de profiling selon le flag `--use-profiling`.
    profiling::set_enabled(use_profiling);

    // Lue avant toute création Vulkan : la shadow map en dépend.
    let config = Arc::new(EngineConfig::load().map_err(|e| {
        logger.error(&format!("Configuration invalide : {e:#}"));
        e
    })?);

    let event_loop = EventLoop::new().map_err(|e| {
        logger.error(&format!("Impossible de créer l'EventLoop : {e}"));
        e
    })?;

    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new(logger.clone(), Box::new(scene), config, requests);
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

/// Nom du binaire (`Physis`, `ai_sandbox`…) : nomme le socket de l'app, que les clients
/// retrouvent par ce nom, et son dossier de captures.
pub(crate) fn app_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.file_stem()?.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "dromon".to_owned())
}
