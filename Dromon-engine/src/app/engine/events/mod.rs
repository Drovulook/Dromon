//! Événements venus de l'extérieur du moteur : clavier/souris (winit) et modifications
//! de fichiers (hot reload).

mod file_watcher;
pub mod input;

pub use file_watcher::{FileChange, FileWatcher};
