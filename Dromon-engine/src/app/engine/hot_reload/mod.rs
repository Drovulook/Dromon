//! **Hot reload** : fichiers modifiés (configs `.ron`, textures du terrain) → ce qu'il
//! faut refaire, appliqué à chaud. Le classement des champs vit dans [`classify`] ; ici,
//! le point d'entrée côté `Engine`, appelé à chaque frame.

mod classify;

pub(crate) use classify::{Reload, Reloads};

use crate::app::engine::Engine;
use crate::app::engine::events::FileChange;
use crate::app::engine::renderer::Renderer;
use crate::config::WorldGenConfig;
use classify::{ConfigReload, Configs};
use std::sync::Arc;

impl Engine {
    /// Traite les fichiers modifiés depuis la frame précédente (cf. `FileWatcher`).
    pub(super) fn handle_file_changes(&mut self) {
        let Some(watcher) = &mut self.file_watcher else {
            return;
        };
        for change in watcher.poll() {
            match change {
                FileChange::Config(file) => self.reload_config(&file),
                FileChange::TerrainTexture(texture) => self.reload_texture(&texture),
            }
        }
    }

    /// Une config invalide ou non applicable à chaud laisse l'ancienne en place.
    fn reload_config(&mut self, file: &str) {
        match classify::reload(&self.config, self.world_gen(), file) {
            Err(e) => self
                .logger
                .error(&format!("{e:#} : ancienne config conservée")),
            Ok(ConfigReload::Unhandled) => {
                self.logger.info(&format!("{file} : hot reload non géré"))
            }
            Ok(ConfigReload::Unchanged) => {}
            Ok(ConfigReload::Unsupported(reloads)) => self.logger.warn(&format!(
                "{file} ignoré, rechargement pas encore géré : {}",
                reloads.join(", ")
            )),
            Ok(ConfigReload::Apply(configs, reloads)) => {
                let Configs { engine, world_gen } = *configs;
                self.config = Arc::new(engine);
                self.apply_reloads(file, world_gen.as_ref(), &reloads);
            }
        }
    }

    /// Fichiers d'une texture du terrain modifiés. Ignorés si aucun matériau ne l'utilise.
    fn reload_texture(&mut self, texture: &str) {
        let reloads = classify::texture_reloads(&self.config, texture);
        if reloads.is_empty() {
            return;
        }
        let world_gen = self.world_gen().cloned();
        self.apply_reloads(&format!("texture {texture}"), world_gen.as_ref(), &reloads);
    }

    /// Config de génération du monde : celle du terrain de la fenêtre principale.
    fn world_gen(&self) -> Option<&WorldGenConfig> {
        self.renderers
            .get(&self.primary_window_id)
            .and_then(Renderer::world_gen)
    }

    /// Applique `reloads` à tous les renderers, avec `self.config` déjà à jour. `what` :
    /// ce qui a été rechargé, pour les logs.
    fn apply_reloads(&mut self, what: &str, world_gen: Option<&WorldGenConfig>, reloads: &Reloads) {
        let mut result = Ok(());
        for renderer in self.renderers.values_mut() {
            result = result.and(renderer.hot_reload(self.config.clone(), world_gen, reloads));
        }
        let done = reloads.describe();
        match result {
            Ok(()) => self.logger.info(&format!("{what} rechargé : {done}")),
            Err(e) => self
                .logger
                .error(&format!("{what} rechargé en partie ({done}) : {e:#}")),
        }
    }
}
