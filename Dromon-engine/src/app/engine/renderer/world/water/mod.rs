mod quadtree;

use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use std::sync::Arc;

use crate::app::engine::renderer::camera::Camera;
use crate::app::engine::renderer::frustum_culling::Frustum;
use crate::app::engine::renderer::render_resources::WaterMesh;
use crate::app::engine::rendering_context::RenderingContext;
use crate::app::engine::terrain_generation::TerrainSource;
use crate::config::{WaterDebug, WaterRenderParams};
use quadtree::{Quadtree, Surface, Viewer};

/// Un carré d'eau à dessiner : une instance de la grille partagée (`WaterMesh`),
/// placée et dimensionnée par push constants.
#[derive(Clone, Copy)]
pub(crate) struct WaterNode {
    /// Coin (x min, y min), en unités monde.
    pub origin: Vec2,
    /// Côté du carré, en unités monde.
    pub size: f32,
    /// Niveau dans le quadtree (0 = le plus fin).
    pub level: u32,
    /// Distances caméra → sommet où le geomorphing commence (`k = 0`) et s'achève
    /// (`k = 1` : le nœud a la forme du niveau au-dessus).
    pub morph_start: f32,
    pub morph_end: f32,
}

/// Bits de `WaterPushConstants::debug_flags`.
pub(crate) const WATER_DEBUG_LOD_COLOR: u32 = 1 << 0;
pub(crate) const WATER_DEBUG_MORPH_COLOR: u32 = 1 << 1;

/// Push constants du shader de l'eau, lues par les deux stages. ⚠ Même disposition
/// que `WaterPushConstants` dans `water.slang`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct WaterPushConstants {
    pub origin: Vec2,
    pub cell_size: f32,
    pub sea_level: f32,
    pub morph_start: f32,
    pub morph_end: f32,
    pub level: u32,
    pub debug_flags: u32,
    pub test_wave_amplitude: f32,
    pub test_wave_length: f32,
    pub max_distance: f32,
}

/// **La surface de la mer** : un plan à `sea_level`, découpé chaque frame en nœuds
/// (quadtree CDLOD) qui redessinent tous la même grille.
pub(crate) struct Water {
    pub sea_level: f32,
    /// Distance horizontale d'affichage (portée de chargement du terrain).
    pub max_distance: f32,
    /// Grille partagée par tous les nœuds, envoyée une fois au démarrage.
    pub mesh: WaterMesh,
    /// Nœuds à dessiner cette frame.
    pub nodes: Vec<WaterNode>,
    pub debug: WaterDebug,
    quadtree: Quadtree,
}

impl Water {
    /// `max_distance` : distance horizontale d'affichage, au-delà de laquelle l'eau est
    /// coupée (nœuds rejetés, puis `discard` au pixel près).
    pub fn new(
        context: Arc<RenderingContext>,
        source: Arc<TerrainSource>,
        sea_level: f32,
        world_radius: f32,
        max_distance: f32,
        params: &WaterRenderParams,
        debug: WaterDebug,
    ) -> Result<Water> {
        Ok(Water {
            sea_level,
            max_distance,
            mesh: WaterMesh::new(context)?,
            nodes: Vec::new(),
            debug,
            quadtree: Quadtree::new(params, world_radius, max_distance, source),
        })
    }

    /// Choisit les nœuds de la frame d'après la caméra (déjà mise à jour).
    pub fn update(&mut self, camera: &Camera) {
        let frustum = Frustum::from_view_proj(camera.proj * camera.view);
        let viewer = Viewer {
            position: camera.position,
            far: camera.params.far,
            frustum: &frustum,
        };
        let surface = Surface {
            sea_level: self.sea_level,
            height_margin: self.debug.test_wave_amplitude.abs(),
        };
        self.quadtree.select(&viewer, &surface, &mut self.nodes);
    }

    /// `debug_flags` des push constants.
    pub fn debug_flags(&self) -> u32 {
        let mut flags = 0;
        if self.debug.lod_color {
            flags |= WATER_DEBUG_LOD_COLOR;
        }
        if self.debug.morph_color {
            flags |= WATER_DEBUG_MORPH_COLOR;
        }
        flags
    }

    /// Copie staging → device de la grille. Passe de transfert du démarrage
    /// uniquement : l'eau est créée dans `Scene::setup`, avant elle.
    pub fn record_upload(&self, command_buffer: &vk::CommandBuffer) {
        self.mesh.record_upload(command_buffer);
    }
}
