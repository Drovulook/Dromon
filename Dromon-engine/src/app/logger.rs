use std::sync::mpsc::SyncSender;

use super::socket_client;

pub struct Logger {
    sender: Option<SyncSender<String>>,
}

impl Logger {
    pub fn new(use_cli: bool) -> Self {
        let sender = if use_cli {
            let (tx, rx) = std::sync::mpsc::sync_channel(1024);
            socket_client::spawn_worker(rx);
            Some(tx)
        } else {
            None
        };
        Self { sender }
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

    fn send(&self, msg: String) {
        match &self.sender {
            Some(tx) => {
                let _ = tx.try_send(msg);
            }
            None => eprintln!("{}", msg),
        }
    }
}
