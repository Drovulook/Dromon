use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use std::sync::Arc;

use crate::app::engine::renderer::render_resources::WaterMesh;
use crate::app::engine::rendering_context::RenderingContext;

/// Un carré d'eau à dessiner : une instance de la grille partagée (`WaterMesh`),
/// placée et dimensionnée par push constants.
#[derive(Clone, Copy)]
pub(crate) struct WaterNode {
    /// Coin (x min, y min), en unités monde.
    pub origin: Vec2,
    /// Côté du carré, en unités monde.
    pub size: f32,
}

/// Push constants du shader de l'eau. ⚠ Même disposition que `WaterPushConstants`
/// dans `water.slang`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct WaterPushConstants {
    pub origin: Vec2,
    pub cell_size: f32,
    pub sea_level: f32,
}

/// **La surface de la mer** : un plan à `sea_level`, découpé en nœuds qui redessinent
/// tous la même grille.
pub(crate) struct Water {
    pub sea_level: f32,
    /// Grille partagée par tous les nœuds, envoyée une fois au démarrage.
    pub mesh: WaterMesh,
    /// Nœuds à dessiner cette frame.
    pub nodes: Vec<WaterNode>,
}

impl Water {
    /// Étape 1 (cf. `Architecture/Rendu-Eau.md`) : un seul nœud fixe couvrant le carré
    /// englobant du monde. Le quadtree CDLOD le remplacera.
    pub fn new(context: Arc<RenderingContext>, sea_level: f32, world_radius: f32) -> Result<Water> {
        Ok(Water {
            sea_level,
            mesh: WaterMesh::new(context)?,
            nodes: vec![WaterNode {
                origin: Vec2::splat(-world_radius),
                size: 2.0 * world_radius,
            }],
        })
    }

    /// Copie staging → device de la grille. Passe de transfert du démarrage
    /// uniquement : l'eau est créée dans `Scene::setup`, avant elle.
    pub fn record_upload(&self, command_buffer: &vk::CommandBuffer) {
        self.mesh.record_upload(command_buffer);
    }
}
