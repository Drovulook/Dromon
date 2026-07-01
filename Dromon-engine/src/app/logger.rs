use std::sync::mpsc::SyncSender;

use super::socket_client;

pub struct Logger {
    sender: Option<SyncSender<String>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Logger {
    pub fn new(use_cli: bool) -> Self {
        let (sender, worker) = if use_cli {
            let (tx, rx) = std::sync::mpsc::sync_channel(1024);
            let handle = socket_client::spawn_worker(rx);
            (Some(tx), Some(handle))
        } else {
            (None, None)
        };
        Self { sender, worker }
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

    /// Un nœud de l'arbre d'init : profondeur, durée cumulée, nombre d'appels
    /// agrégés et nom du bloc. Le `name` est placé en dernier car il peut contenir
    /// des espaces.
    pub fn profile_init(&self, depth: usize, name: &str, total_us: u128, count: u32) {
        self.send(format!(
            "[PROFILE_INIT] depth={} dur_us={} count={} name={}",
            depth, total_us, count, name
        ));
    }

    fn send(&self, msg: String) {
        match &self.sender {
            Some(tx) => {
                let _ = tx.try_send(msg);
            }
            None => eprintln!("{}", msg),
        }
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        // Déconnecte le channel pour signaler la fin au worker,
        // puis attend qu'il ait expédié tous les messages en attente.
        drop(self.sender.take());
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}
