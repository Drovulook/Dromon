mod mesh_job;
mod terrain;

use crate::app::engine::renderer::camera::Camera;
use crate::app::engine::renderer::light::{DirectionalLight, ShadowConfig};
use crate::app::engine::terrain_generation::{LodUpdater, MeshCache};
use crate::app::engine::{inputs::InputState, terrain_generation::ChunkManager};
use crate::profile;
use anyhow::Result;
use ash::vk;
use std::collections::HashMap;
use std::sync::Arc;

use crate::app::{
    engine::{
        renderer::{
            descriptors::DescriptorHandler,
            render_resources::{RenderObject, RenderResourceManager, TerrainMesh},
            world::mesh_job::MeshJob,
        },
        rendering_context::RenderingContext,
        timer::Timer,
    },
    logger::Logger,
};
use glam::IVec2;

pub struct World {
    pub logger: Arc<Logger>,
    pub rrm: RenderResourceManager,
    pub render_objects: Vec<RenderObject>,
    pub camera: Camera,
    pub light: DirectionalLight,
    /// Données du terrain (relief + édits). `None` tant que la scène n'a pas appelé
    /// [`World::generate_terrain`]. En `Arc` car **immuable une fois généré** : les
    /// threads de maillage en partagent la lecture sans copie ni verrou.
    pub chunk_manager: Option<Arc<ChunkManager>>,
    /// Un mesh GPU par chunk **non vide**, indexé par coordonnée : c'est ce qui permet
    /// de remplacer le mesh d'un chunk précis quand son LOD change (un `Vec` n'avait
    /// aucun lien avec les positions, les chunks vides décalant même les index).
    pub terrain_meshes: HashMap<IVec2, TerrainMesh>,
    /// Politique de LOD suivant la caméra. `None` tant qu'il n'y a pas de terrain.
    lod_updater: Option<LodUpdater>,
    /// Altitude moyenne du relief : plan de référence de la composante « hauteur de
    /// caméra » du LOD (cf. [`LodFocus`]).
    terrain_reference_z: f32,
    /// Géométries déjà calculées, gardées pour les allers-retours de caméra.
    mesh_cache: MeshCache,
    /// Lot de re-maillage en cours, s'il y en a un.
    mesh_job: Option<MeshJob>,
    /// Chunks dont les buffers attendent leur copie staging → device.
    pending_uploads: Vec<IVec2>,
    /// Meshes remplacés, avec la frame de mise au rebut. `Buffer::drop` détruit le
    /// buffer Vulkan **immédiatement** : libérer tout de suite un mesh encore
    /// référencé par une frame en vol produirait une erreur de validation, voire un
    /// crash. On attend donc que toutes les frames concernées soient terminées.
    graveyard: Vec<(TerrainMesh, u64)>,
    /// Compteur de frames, base du délai du cimetière.
    frame: u64,
    /// Nombre de frames que le renderer garde en vol.
    frames_in_flight: u64,
    /// Contexte GPU, conservé pour pouvoir allouer les buffers du terrain au
    /// moment du `setup` (la génération est pilotée par la scène) comme en cours de jeu.
    context: Arc<RenderingContext>,
}

impl World {
    /// Crée un monde *vide* : seul le gestionnaire de ressources (`rorm`) et des
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
        let rorm = RenderResourceManager::new(context.clone(), logger.clone(), descriptor_handler)?;

        Ok(World {
            logger,
            rrm: rorm,
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
            chunk_manager: None,
            terrain_meshes: HashMap::new(),
            lod_updater: None,
            terrain_reference_z: 0.0,
            mesh_cache: MeshCache::default(),
            mesh_job: None,
            pending_uploads: Vec::new(),
            graveyard: Vec::new(),
            frame: 0,
            frames_in_flight: frames_in_flight as u64,
            context,
        })
    }

    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        profile!();
        self.rrm.initialize(command_buffer)?;
        for mesh in self.terrain_meshes.values() {
            mesh.record_upload(command_buffer);
        }
        Ok(())
    }

    pub fn update_world_data(&mut self, timer: &Timer, input_state: &InputState, aspect: f32) {
        profile!();
        self.camera.update(input_state, timer, aspect);
    }
}
