pub(crate) mod atmosphere;
mod day_night;
pub(crate) mod game_clock;
pub(crate) mod light;
pub(crate) mod stars;
pub(crate) mod terrain;

use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use crate::app::engine::inputs::InputState;
use crate::app::engine::renderer::world::atmosphere::Atmosphere;
use crate::app::engine::renderer::world::light::{DirectionalLight, sun_direction};
use crate::app::engine::renderer::world::stars::{Stars, world_to_sky};
use crate::app::engine::renderer::world::terrain::Terrain;
use crate::app::{
    engine::{
        renderer::{
            camera::Camera,
            descriptors::DescriptorHandler,
            render_resources::{RenderObject, RenderResourceManager},
        },
        rendering_context::RenderingContext,
        timer::Timer,
    },
    logger::Logger,
};
use crate::config::EngineConfig;
use crate::profile;
use game_clock::GameClock;

/// Le contenu de la scène : ce que l'application décrit via le trait
/// [`Scene`](crate::Scene), plus le terrain quand il y en a un.
pub struct World {
    pub logger: Arc<Logger>,
    pub rrm: RenderResourceManager,
    pub render_objects: Vec<RenderObject>,
    pub camera: Camera,
    pub light: DirectionalLight,
    pub atmosphere: Atmosphere,
    pub stars: Stars,
    /// Le terrain vivant. `None` tant que la scène n'a pas appelé
    /// [`World::generate_terrain`] — une scène n'est pas obligée d'en avoir un.
    pub(crate) terrain: Option<Terrain>,
    /// Contexte GPU, conservé pour pouvoir allouer les buffers du terrain au
    /// moment du `setup` (la génération est pilotée par la scène) comme en cours de jeu.
    context: Arc<RenderingContext>,
    /// Nombre de frames que le renderer garde en vol — le terrain en a besoin pour dater
    /// la destruction différée de ses meshes.
    frames_in_flight: u64,
    game_clock: GameClock,
    /// Latitude (radians) : fixe la hauteur du soleil à midi (`90° - latitude`).
    pub latitude: f32,
    /// Configs moteur lues au démarrage (`render.ron`, `environment.ron`).
    pub(crate) config: Arc<EngineConfig>,
}

impl World {
    /// Crée un monde *vide* : seul le gestionnaire de ressources (`rrm`) et des
    /// valeurs par défaut (caméra, lumière) sont initialisés. Le contenu concret
    /// (assets + `RenderObject`, terrain) est fourni par la `Scene` via
    /// `Scene::setup`, appelée juste après la construction du `Renderer`. La
    /// scène peut aussi modifier `world.light` à ce moment-là.
    pub(crate) fn new(
        logger: Arc<Logger>,
        context: Arc<RenderingContext>,
        descriptor_handler: Arc<DescriptorHandler>,
        frames_in_flight: usize,
        config: Arc<EngineConfig>,
    ) -> Result<World> {
        let rrm = RenderResourceManager::new(context.clone(), logger.clone(), descriptor_handler)?;
        let env = &config.environment;

        let mut world = World {
            logger,
            rrm,
            render_objects: Vec::new(),
            camera: Camera::new(&config.render.camera),
            light: DirectionalLight::new(&config),
            atmosphere: Atmosphere::new(env.atmosphere),
            stars: Stars::new(env.stars),
            terrain: None,
            context,
            frames_in_flight: frames_in_flight as u64,
            game_clock: GameClock::new(&env.clock),
            latitude: env.latitude_deg.to_radians(),
            config,
        };
        // Soleil et ciel cohérents avec l'heure de départ dès `Scene::setup`.
        world.sync_sky();
        Ok(world)
    }

    /// Recalcule soleil, couleurs du ciel et étoiles d'après l'heure courante.
    fn sync_sky(&mut self) {
        let time_of_day = self.game_clock.time_of_day;
        self.light.direction = sun_direction(time_of_day, self.latitude);
        self.stars.world_to_sky = world_to_sky(time_of_day, self.latitude);
        day_night::apply(
            &self.config.environment.day_night,
            &mut self.light,
            &mut self.atmosphere,
            &mut self.stars,
        );
    }

    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        profile!();
        // Pas de terrain ici : ses meshes arrivent au fil des frames (`update_terrain`).
        self.rrm.initialize(command_buffer)
    }

    pub fn update_world_data(&mut self, timer: &Timer, input_state: &InputState, aspect: f32) {
        profile!();
        self.game_clock.advance(timer.delta_secs() as f64);
        self.sync_sky();
        self.stars.time_secs = timer.elapsed_secs();
        self.camera.update(input_state, timer, aspect);
        if let Some(terrain) = self.terrain.as_mut() {
            terrain.update_visibility(&self.camera, &self.light);
        }
    }

    /// Fait suivre le terrain aux mouvements de caméra (LOD et streaming). Sans terrain,
    /// ne fait rien.
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
