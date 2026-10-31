//! **Captures d'écran** côté `Engine` : qui les demande et où elles vont. La relecture de
//! l'image elle-même : `renderer::screenshot`.
//! - F12 → `work/captures/`, gardées.
//! - Commande `screenshot` (agent) → [`CaptureDir`], dossier temporaire.

use crate::AppDirs;
use crate::app::engine::Engine;
use crate::app::engine::commands::Responder;
use crate::app::engine::renderer::screenshot::ScreenshotRequest;
use crate::app::socket_server::runtime_dir;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use winit::window::WindowId;

/// Captures demandées par commande : `<runtime_dir>/<app>-<pid>/`, supprimé quand le
/// moteur s'arrête (`Drop`). Un moteur tué ne passe pas par `Drop` : son dossier est
/// supprimé au démarrage suivant (cf. [`remove_stale`]).
pub(super) struct CaptureDir {
    path: PathBuf,
    count: u32,
}

impl CaptureDir {
    pub fn new(app: &str) -> Self {
        let root = runtime_dir();
        remove_stale(&root);
        Self {
            path: root.join(format!("{app}-{}", std::process::id())),
            count: 0,
        }
    }

    /// `screenshot-001.png`, `-002`… (dossier créé à l'écriture).
    fn next_path(&mut self) -> PathBuf {
        self.count += 1;
        self.path.join(format!("screenshot-{:03}.png", self.count))
    }
}

impl Drop for CaptureDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Supprime les dossiers `<nom>-<pid>` de `root` dont le processus n'existe plus
/// (`/proc/<pid>` absent).
fn remove_stale(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let pid = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.rsplit_once('-'))
            .and_then(|(_, pid)| pid.parse::<u32>().ok());
        if let Some(pid) = pid
            && path.is_dir()
            && !Path::new("/proc").join(pid.to_string()).exists()
        {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

impl Engine {
    /// Capture gardée dans `work/captures/`, nommée par l'heure (ms Unix).
    pub(super) fn manual_screenshot(&mut self, window_id: WindowId) {
        let Some(renderer) = self.renderers.get_mut(&window_id) else {
            return;
        };
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = AppDirs::get()
            .work()
            .join("captures")
            .join(format!("capture-{millis}.png"));
        let logger = self.logger.clone();
        renderer.request_screenshot(ScreenshotRequest {
            path,
            done: Box::new(move |result| match result {
                Ok(path) => logger.info(&format!("capture écrite : {}", path.display())),
                Err(e) => logger.error(&format!("capture : {e:#}")),
            }),
        });
    }

    /// Commande `screenshot` : la réponse (`{"path": …}`) part du thread qui écrit le
    /// PNG, quelques frames plus tard.
    pub(super) fn command_screenshot(&mut self, responder: Responder) {
        let path = self.capture_dir.next_path();
        let Some(renderer) = self.renderers.get_mut(&self.primary_window_id) else {
            return responder.send(Err("pas de fenêtre principale".to_owned()));
        };
        renderer.request_screenshot(ScreenshotRequest {
            path,
            done: Box::new(move |result| {
                responder.send(
                    result
                        .map(|path| json!({ "path": path }))
                        .map_err(|e| format!("{e:#}")),
                );
            }),
        });
    }
}
