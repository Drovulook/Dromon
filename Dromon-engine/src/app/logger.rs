use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Weak};

use super::socket_server::{self, Incoming, Outgoing};

pub struct Logger {
    sender: Option<SyncSender<Outgoing>>,
    /// Sans CLI : logs aussi sur stderr. Avec, jamais : ils écriraient sur le TUI.
    stderr: bool,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Logger {
    /// Ouvre le socket de l'app `app` (logs diffusés + commandes), avec ou sans CLI.
    /// `use_cli` : attend sa connexion (bloquant, 2 s max).
    /// Renvoie aussi les lignes reçues des clients (commandes), `None` sans socket.
    pub fn new(app: &str, use_cli: bool) -> (Self, Option<Receiver<Incoming>>) {
        let path = socket_server::socket_path(app);
        match socket_server::spawn_worker(path, use_cli) {
            Ok(server) => {
                if use_cli && !server.client_connected {
                    eprintln!("[WARN] CLI absent au démarrage : premiers logs perdus");
                }
                let logger = Self {
                    sender: Some(server.outgoing),
                    stderr: !use_cli,
                    worker: Some(server.worker),
                };
                (logger, Some(server.incoming))
            }
            Err(e) => {
                eprintln!("[WARN] socket indisponible ({e}) : logs sur stderr, pas de commandes");
                let logger = Self {
                    sender: None,
                    stderr: true,
                    worker: None,
                };
                (logger, None)
            }
        }
    }

    /// Les paniques (tous threads) passent par le logger au lieu de stderr, qui
    /// écrirait par-dessus le TUI. `Weak` : un `Arc` gardé par le hook empêcherait
    /// le `Drop` qui vide les derniers messages.
    pub fn install_panic_hook(logger: &Arc<Logger>) {
        let logger: Weak<Logger> = Arc::downgrade(logger);
        std::panic::set_hook(Box::new(move |info| {
            let thread = std::thread::current();
            let msg = format!("panic (thread {}) : {info}", thread.name().unwrap_or("?"));
            match logger.upgrade() {
                Some(logger) => logger.error(&msg),
                None => eprintln!("{msg}"),
            }
        }));
    }

    pub fn info(&self, msg: &str) {
        self.send(format!("[INFO] {}", msg));
    }

    pub fn warn(&self, msg: &str) {
        self.send(format!("[WARN] {}", msg));
    }

    pub fn error(&self, msg: &str) {
        self.send(format!("[ERROR] {}", msg));
    }

    pub fn vulkan_layer_msg(&self, msg: &str) {
        self.send(format!("[VULKAN] {}", msg));
    }

    pub fn fps(&self, fps: f32) {
        self.send(format!("[FPS] {:.1}", fps));
    }

    pub fn state(&self, s: &str) {
        self.send(format!("[STATE] {}", s));
    }

    /// Envoie un arbre de profiling en un message. `kind` (`INIT`, `FRAME`…) → tag
    /// `[PROFILE_<KIND>]`. `payload` : nœuds séparés par `\x1e`, champs par `\x1f`.
    pub fn profile(&self, kind: &str, payload: &str) {
        self.send(format!("[PROFILE_{}] {}", kind, payload));
    }

    /// Envoie les statistiques du terrain (onglet « world » du CLI). Même
    /// convention d'encodage que [`Logger::profile`] : un enregistrement par
    /// niveau de LOD séparé par `\x1e`, champs `lod␟chunks␟sommets` par `\x1f`.
    pub fn world(&self, payload: &str) {
        self.send(format!("[WORLD] {}", payload));
    }

    fn send(&self, msg: String) {
        if self.stderr {
            eprintln!("{msg}");
        }
        if let Some(tx) = &self.sender {
            let _ = tx.try_send(Outgoing::Broadcast(msg));
        }
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        // `Shutdown` passe après les messages en attente : le worker les expédie
        // tous avant de s'arrêter.
        if let Some(tx) = self.sender.take() {
            let _ = tx.send(Outgoing::Shutdown);
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}
