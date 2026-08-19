pub(crate) mod terrain;

use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use crate::app::engine::inputs::InputState;
use crate::app::engine::renderer::world::terrain::Terrain;
use crate::app::{
    engine::{
        renderer::{
            camera::Camera,
            descriptors::DescriptorHandler,
            light::{DirectionalLight, ShadowConfig},
            render_resources::{RenderObject, RenderResourceManager},
        },
        rendering_context::RenderingContext,
        timer::Timer,
    },
    logger::Logger,
};
use crate::profile;

/// Le contenu de la scène : ce que l'application décrit via le trait
/// [`Scene`](crate::Scene), plus le terrain quand il y en a un.
pub struct World {
    pub logger: Arc<Logger>,
    pub rrm: RenderResourceManager,
    pub render_objects: Vec<RenderObject>,
    pub camera: Camera,
    pub light: DirectionalLight,
    /// Le terrain vivant. `None` tant que la scène n'a pas appelé
    /// [`World::generate_terrain`] — une scène n'est pas obligée d'en avoir un.
    pub(crate) terrain: Option<Terrain>,
    /// Contexte GPU, conservé pour pouvoir allouer les buffers du terrain au
    /// moment du `setup` (la génération est pilotée par la scène) comme en cours de jeu.
    context: Arc<RenderingContext>,
    /// Nombre de frames que le renderer garde en vol — le terrain en a besoin pour dater
    /// la destruction différée de ses meshes.
    frames_in_flight: u64,
}

impl World {
    /// Crée un monde *vide* : seul le gestionnaire de ressources (`rrm`) et des
    /// valeurs par défaut (caméra, lumière) sont initialisés. Le contenu concret
    /// (assets + `RenderObject`, terrain) est fourni par la `Scene` via
    /// `Scene::setup`, appelée juste après la construction du `Renderer`. La
    /// scène peut aussi modifier `world.light` à ce moment-là.
    pub fn new(
        logger: Arc<Logger>,
        context: Arc<RenderingContext>,
        descriptor_handler: Arc<DescriptorHandler>,
        frames_in_flight: usize,
    ) -> Result<World> {
        let rrm = RenderResourceManager::new(context.clone(), logger.clone(), descriptor_handler)?;

        Ok(World {
            logger,
            rrm,
            render_objects: Vec::new(),
            camera: Camera::default(),
            light: DirectionalLight {
                direction: glam::Vec3::new(-0.3, -0.5, -1.0),
                color: glam::Vec3::ONE,
                intensity: 1.0,
                // Défaut « petite scène » : boîte fixe à l'origine. `generate_terrain`
                // bascule en mode terrain si la scène crée un terrain.
                shadow: ShadowConfig::default(),
            },
            terrain: None,
            context,
            frames_in_flight: frames_in_flight as u64,
        })
    }

    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        profile!();
        self.rrm.initialize(command_buffer)?;
        if let Some(terrain) = self.terrain.as_ref() {
            terrain.initialize(command_buffer);
        }
        Ok(())
    }

    pub fn update_world_data(&mut self, timer: &Timer, input_state: &InputState, aspect: f32) {
        profile!();
        self.camera.update(input_state, timer, aspect);
        if let Some(terrain) = self.terrain.as_mut() {
            terrain.update_visibility(&self.camera, &self.light);
        }
    }

    /// Fait suivre le terrain aux mouvements de caméra (LOD aujourd'hui, streaming
    /// demain). Sans terrain, ne fait rien.
    pub fn update_terrain(&mut self) -> Result<()> {
        let Some(terrain) = self.terrain.as_mut() else {
            return Ok(());
        };
        terrain.update(&self.camera)
    }

    /// Copies staging → device des meshes de terrain fraîchement installés.
    pub fn record_terrain_uploads(&mut self, command_buffer: vk::CommandBuffer) {
        if let Some(terrain) = self.terrain.as_mut() {
            terrain.record_uploads(command_buffer);
        }
    }
}
