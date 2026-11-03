//! Surveillance des fichiers pour le hot reload : configs (`config/*.ron`) et cartes des
//! textures du terrain, fournies (`res/`) ou générées (`work/texgen/`). Le thread de
//! `notify` pousse les événements bruts dans un channel ; l'`Engine` le vide à chaque
//! frame via [`FileWatcher::poll`].

use crate::AppDirs;
use crate::app::logger::Logger;
use crate::config::{TEXTURE_MAPS, generated_texture_dir, terrain_texture_dir};
use anyhow::{Context, Result};
use notify::event::{AccessKind, AccessMode, ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// Délai de calme avant de signaler un changement : regroupe la rafale d'événements
/// d'une sauvegarde, ou les cartes d'une texture exportées ensemble.
const DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FileChange {
    /// Fichier du dossier `config/` de l'app, ex. `"render.ron"`.
    Config(String),
    /// Texture `name` dont au moins une carte a changé : sous-dossier de
    /// [`generated_texture_dir`] si `generated`, sinon de [`terrain_texture_dir`].
    TerrainTexture { name: String, generated: bool },
}

pub struct FileWatcher {
    // Gardé vivant : le drop arrête la surveillance.
    _watcher: RecommendedWatcher,
    receiver: Receiver<notify::Result<Event>>,
    // Canonicalisés : notify rapporte les chemins tels qu'on les a donnés, et un `..`
    // ou un lien symbolique fausserait les comparaisons.
    config_dir: PathBuf,
    texture_dir: PathBuf,
    generated_dir: PathBuf,
    /// Changements en attente, avec l'instant de leur dernier événement.
    pending: HashMap<FileChange, Instant>,
    logger: Arc<Logger>,
}

impl FileWatcher {
    pub fn new(logger: Arc<Logger>) -> Result<Self> {
        let config_dir =
            std::fs::canonicalize(AppDirs::get().config()).context("dossier config/")?;
        // Créés au besoin : une app sans terrain texturé (model_sandbox) n'en a pas, et
        // work/texgen/ n'existe qu'après une première génération.
        let texture_dir = existing_dir(terrain_texture_dir())?;
        let generated_dir = existing_dir(generated_texture_dir())?;

        let (sender, receiver) = channel();
        let mut watcher = notify::recommended_watcher(sender)?;
        // On surveille les dossiers, pas les fichiers : un renommage par-dessus un
        // fichier (écriture atomique) remplace son inode et tuerait un watch de fichier.
        watcher.watch(&config_dir, RecursiveMode::NonRecursive)?;
        watcher.watch(&texture_dir, RecursiveMode::Recursive)?;
        watcher.watch(&generated_dir, RecursiveMode::Recursive)?;

        Ok(Self {
            _watcher: watcher,
            receiver,
            config_dir,
            texture_dir,
            generated_dir,
            pending: HashMap::new(),
            logger,
        })
    }

    /// Changements stables depuis au moins [`DEBOUNCE`]. Non bloquant.
    pub fn poll(&mut self) -> Vec<FileChange> {
        let now = Instant::now();
        while let Ok(result) = self.receiver.try_recv() {
            match result {
                Ok(event) if is_write_done(&event.kind) => {
                    for path in &event.paths {
                        if let Some(change) = self.classify(path) {
                            self.pending.insert(change, now);
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => self
                    .logger
                    .warn(&format!("surveillance des fichiers : {e}")),
            }
        }

        let ready: Vec<FileChange> = self
            .pending
            .iter()
            .filter(|(_, last)| now.duration_since(**last) >= DEBOUNCE)
            .map(|(change, _)| change.clone())
            .collect();
        for change in &ready {
            self.pending.remove(change);
        }
        ready
    }

    /// Fichier connu → changement ; le reste (fichiers temporaires, `.blend`…) → `None`.
    fn classify(&self, path: &Path) -> Option<FileChange> {
        let name = path.file_name()?.to_str()?;
        if path.parent()? == self.config_dir {
            return name
                .ends_with(".ron")
                .then(|| FileChange::Config(name.to_owned()));
        }
        // Attendu : <racine>/<texture>/<carte>, la racine étant celle des textures
        // fournies ou générées.
        let dir = path.parent()?;
        let (texture, generated) = match dir.strip_prefix(&self.generated_dir) {
            Ok(texture) => (texture, true),
            Err(_) => (dir.strip_prefix(&self.texture_dir).ok()?, false),
        };
        let texture = texture.to_str()?;
        (!texture.is_empty() && !texture.contains('/') && TEXTURE_MAPS.contains(&name)).then(
            || FileChange::TerrainTexture {
                name: texture.to_owned(),
                generated,
            },
        )
    }
}

/// `dir`, créé au besoin, canonicalisé.
fn existing_dir(dir: PathBuf) -> Result<PathBuf> {
    std::fs::create_dir_all(&dir).with_context(|| format!("dossier {}", dir.display()))?;
    std::fs::canonicalize(&dir).with_context(|| format!("dossier {}", dir.display()))
}

/// Écriture terminée : fichier refermé après écriture en place (Neovim), ou renommé
/// vers son nom final (écriture atomique : Claude, GIMP, git…). On ignore `MODIFY`,
/// émis en cours d'écriture, quand le fichier peut encore être incomplet.
fn is_write_done(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Access(AccessKind::Close(AccessMode::Write))
            | EventKind::Modify(ModifyKind::Name(RenameMode::To))
    )
}
