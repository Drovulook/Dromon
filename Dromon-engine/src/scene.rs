use crate::{Timer, World};
use anyhow::Result;

/// Frontière moteur ↔ application. Une `Scene` décrit *quoi* afficher et *comment*
/// ça évolue ; le moteur (`Engine`/`Renderer`) gère le *comment* du rendu Vulkan.
///
/// L'application (ex. `model_sandbox`) implémente ce trait, et le moteur l'appelle
/// au bon moment via `dromon_engine::run(ma_scene)`. C'est l'équivalent Rust d'une
/// classe de base `Game` virtuelle en C++ : le moteur tient la boucle, la scène
/// fournit le contenu.
pub trait Scene {
    /// Appelé une seule fois, après la création du contexte GPU et AVANT l'upload
    /// des données sur le GPU. C'est ici qu'on charge les assets (`world.rorm`) et
    /// qu'on crée les `RenderObject` initiaux (`world.render_objects`).
    fn setup(&mut self, world: &mut World) -> Result<()>;

    /// Appelé à chaque frame, juste avant le rendu. C'est ici qu'on fait évoluer
    /// la scène dans le temps (déplacer/tourner des objets, etc.).
    fn update(&mut self, world: &mut World, timer: &Timer);
}
