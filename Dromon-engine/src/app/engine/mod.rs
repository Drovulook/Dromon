mod debug_messenger;
mod events;
mod hot_reload;
pub mod renderer;
mod rendering_context;
pub mod terrain_generation;
pub mod timer;

use crate::Scene;
use crate::app::engine::events::input::{self, base_event_subscriber};
use crate::app::engine::events::{FileChange, FileWatcher};
use crate::app::engine::hot_reload::ConfigReload;
use crate::app::engine::rendering_context::{
    ContextAttributes, RenderingContext, queue_family_picker,
};
use crate::app::engine::timer::Timer;
use crate::app::logger::Logger;
use crate::config::EngineConfig;
use crate::profile;
use anyhow::Result;
use renderer::Renderer;
use std::{collections::HashMap, sync::Arc};
use winit::{
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowId},
};

#[cfg(target_os = "linux")]
use winit::platform::wayland::WindowAttributesExtWayland;

pub struct Engine {
    renderers: HashMap<WindowId, Renderer>,
    windows: HashMap<WindowId, Arc<Window>>,
    primary_window_id: WindowId,
    rendering_context: Arc<RenderingContext>,
    input_manager: input::InputManager,
    /// `None` si la surveillance n'a pas pu démarrer : le moteur tourne sans hot reload.
    file_watcher: Option<FileWatcher>,
    logger: Arc<Logger>,
    timer: Timer,
    config: Arc<EngineConfig>,
}

impl Engine {
    pub fn new(
        event_loop: &ActiveEventLoop,
        logger: Arc<Logger>,
        scene: &mut dyn Scene,
        config: Arc<EngineConfig>,
    ) -> Result<Self> {
        crate::profiling::initialize::begin();
        crate::profile!();
        let attrs = WindowAttributes::default().with_title("Dromon");
        #[cfg(target_os = "linux")]
        let attrs = attrs.with_name("dromon", "dromon");
        let primary_window = Arc::new(event_loop.create_window(attrs)?);

        let primary_window_id = primary_window.id();

        let rendering_context = Arc::new(RenderingContext::new(
            logger.clone(),
            ContextAttributes {
                compatibility_window: &primary_window,
                queue_family_picker: queue_family_picker::single_queue_family,
            },
        )?);

        let mut input_manager = input::InputManager::new();
        base_event_subscriber(&mut input_manager, logger.clone());

        let file_watcher = FileWatcher::new(logger.clone())
            .inspect_err(|e| logger.warn(&format!("hot reload désactivé : {e:#}")))
            .ok();

        let windows = HashMap::from([(primary_window_id, primary_window.clone())]);

        logger.state("Initializing");

        let renderers = windows
            .iter()
            .map(|(id, window)| {
                let renderer = Renderer::new(
                    rendering_context.clone(),
                    window.clone(),
                    logger.clone(),
                    scene,
                    config.clone(),
                )?;
                Ok((*id, renderer))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        logger.state("Running");

        Ok(Self {
            renderers,
            windows,
            primary_window_id,
            rendering_context,
            input_manager,
            file_watcher,
            logger,
            timer: Timer::new(),
            config,
        })
    }

    pub fn request_redraw(&self) {
        for window in self.windows.values() {
            window.request_redraw();
        }
    }

    pub(crate) fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: winit::event::WindowEvent,
        scene: &mut dyn Scene,
    ) -> Result<()> {
        self.input_manager.handle_window_event(&event);
        match event {
            winit::event::WindowEvent::CloseRequested => {
                self.logger.state("Closing");
                if window_id == self.primary_window_id {
                    event_loop.exit();
                } else {
                    self.windows.remove(&window_id);
                    self.renderers.remove(&window_id);
                }
                self.logger.state("Closed");
            }

            winit::event::WindowEvent::RedrawRequested => {
                crate::profiling::frame::begin();
                {
                    profile!();
                    self.handle_file_changes();
                    if let Some(renderer) = self.renderers.get_mut(&window_id) {
                        renderer.render(&self.timer, self.input_manager.input_state(), scene)?;
                    }
                    self.input_manager.end_frame();
                    self.timer.tick();
                }
                self.logger.fps(self.timer.fps());
                crate::profiling::frame::end(&self.logger);
            }

            winit::event::WindowEvent::Resized(_size) => {
                if let Some(renderer) = self.renderers.get_mut(&window_id) {
                    renderer.resize();
                }
            }

            winit::event::WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(renderer) = self.renderers.get_mut(&window_id) {
                    renderer.resize();
                }
            }

            _ => {}
        }
        Ok(())
    }

    fn handle_file_changes(&mut self) {
        let Some(watcher) = &mut self.file_watcher else {
            return;
        };
        for change in watcher.poll() {
            match change {
                FileChange::Config(file) => self.reload_config(&file),
                FileChange::TerrainTexture(texture) => self.logger.info(&format!(
                    "texture {texture} modifiée : hot reload pas encore géré"
                )),
            }
        }
    }

    /// Une config invalide ou non applicable à chaud laisse l'ancienne en place.
    fn reload_config(&mut self, file: &str) {
        match hot_reload::reload(&self.config, file) {
            Err(e) => self
                .logger
                .error(&format!("{e:#} : ancienne config conservée")),
            Ok(ConfigReload::NotEngine) => self.logger.info(&format!(
                "{file} : config de scène, hot reload pas encore géré"
            )),
            Ok(ConfigReload::Unchanged) => {}
            Ok(ConfigReload::Unsupported(reloads)) => self.logger.warn(&format!(
                "{file} ignoré, rechargement pas encore géré : {}",
                reloads.join(", ")
            )),
            Ok(ConfigReload::Apply(config, reloads)) => {
                self.config = Arc::from(config);
                let mut result = Ok(());
                for renderer in self.renderers.values_mut() {
                    result = result.and(renderer.hot_reload(self.config.clone(), &reloads));
                }
                match result {
                    Ok(()) => self.logger.info(&format!("{file} rechargé")),
                    Err(e) => self
                        .logger
                        .error(&format!("{file} rechargé en partie : {e:#}")),
                }
            }
        }
    }

    pub fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        self.input_manager.handle_device_event(&event);
    }

    pub fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
        logger: Arc<Logger>,
        scene: &mut dyn Scene,
    ) -> Result<WindowId> {
        let window = Arc::new(event_loop.create_window(attributes)?);
        let window_id = window.id();
        self.windows.insert(window_id, window.clone());
        self.renderers.insert(
            window_id,
            Renderer::new(
                self.rendering_context.clone(),
                window.clone(),
                logger,
                scene,
                self.config.clone(),
            )?,
        );
        Ok(window_id)
    }
}
