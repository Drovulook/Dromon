use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Weak};

use super::socket_server::{self, Incoming, Outgoing};

pub struct Logger {
    sender: Option<SyncSender<Outgoing>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Logger {
    /// `use_cli` : attend la connexion du CLI (bloquant, 2 s max), sinon logs sur stderr.
    /// Renvoie aussi les lignes reçues des clients (commandes), `None` sans socket.
    pub fn new(use_cli: bool) -> (Self, Option<Receiver<Incoming>>) {
        let mut logger = Self {
            sender: None,
            worker: None,
        };
        if !use_cli {
            return (logger, None);
        }
        match socket_server::spawn_worker() {
            Ok((worker, sender, incoming)) => {
                logger.sender = Some(sender);
                logger.worker = Some(worker);
                (logger, Some(incoming))
            }
            Err(e) => {
                eprintln!("[WARN] socket des logs indisponible ({e}) : logs sur stderr");
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
        match &self.sender {
            Some(tx) => {
                let _ = tx.try_send(Outgoing::Broadcast(msg));
            }
            None => eprintln!("{}", msg),
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
