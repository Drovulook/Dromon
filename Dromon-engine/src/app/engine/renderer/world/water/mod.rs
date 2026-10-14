mod quadtree;
mod waves;

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
use crate::config::{WaterParams, WaterRenderParams};
use quadtree::{Quadtree, Surface, Viewer};
pub(crate) use waves::{WAVE_COUNT, Waves};

/// Marge (voxels) au-delà de laquelle un point est « sur terre », sans eau : erreur de
/// la distance à la côte (< 1 cellule macro de 32) plus le bruit de détail qui déplace
/// le trait de côte. Rejet des nœuds (quadtree) et masque au pixel (shaders, via l'UBO).
pub(crate) const LAND_MARGIN: f32 = 32.0;

/// Un carré d'eau à dessiner : une instance de la grille partagée (`WaterMesh`),
/// placée et dimensionnée par push constants.
#[derive(Clone, Copy)]
pub(crate) struct WaterNode {
    /// Coin (x min, y min), en unités monde.
    pub origin: Vec2,
    /// Côté du carré, en unités monde.
    pub size: f32,
    /// Distances caméra → sommet où le geomorphing commence (`k = 0`) et s'achève
    /// (`k = 1` : le nœud a la forme du niveau au-dessus).
    pub morph_start: f32,
    pub morph_end: f32,
}

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
}

/// **La surface de la mer** : un plan à `sea_level`, découpé chaque frame en nœuds
/// (quadtree CDLOD) qui redessinent tous la même grille.
pub(crate) struct Water {
    pub sea_level: f32,
    /// Distance horizontale d'affichage (portée de chargement du terrain).
    pub max_distance: f32,
    /// Aspect de l'eau (`environment.ron`), envoyé dans l'UBO.
    pub params: WaterParams,
    /// Houle, tirée de `params.waves`.
    pub waves: Waves,
    /// Grille partagée par tous les nœuds, envoyée une fois au démarrage.
    pub mesh: WaterMesh,
    /// Nœuds à dessiner cette frame.
    pub nodes: Vec<WaterNode>,
    /// Caméra sous la surface, en mer (même critère que `seaCovers` des shaders).
    pub camera_under_sea: bool,
    quadtree: Quadtree,
    source: Arc<TerrainSource>,
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
        render_params: &WaterRenderParams,
        params: WaterParams,
    ) -> Result<Water> {
        Ok(Water {
            sea_level,
            max_distance,
            params,
            waves: Waves::new(&params.waves),
            mesh: WaterMesh::new(context)?,
            nodes: Vec::new(),
            camera_under_sea: false,
            quadtree: Quadtree::new(render_params, world_radius, max_distance, source.clone()),
            source,
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
            height_margin: self.waves.max_amplitude(),
        };
        self.quadtree.select(&viewer, &surface, &mut self.nodes);

        let p = camera.position;
        self.camera_under_sea =
            p.z < self.sea_level && self.source.coast_distance(p.x, p.y) <= LAND_MARGIN;
    }

    /// Copie staging → device de la grille. Passe de transfert du démarrage
    /// uniquement : l'eau est créée dans `Scene::setup`, avant elle.
    pub fn record_upload(&self, command_buffer: &vk::CommandBuffer) {
        self.mesh.record_upload(command_buffer);
    }
}
