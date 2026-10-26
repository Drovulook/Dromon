pub(crate) mod atmosphere;
mod day_night;
pub(crate) mod game_clock;
pub(crate) mod light;
pub(crate) mod stars;
pub(crate) mod terrain;
pub(crate) mod water;

use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use crate::app::engine::events::input::InputState;
use crate::app::engine::renderer::world::atmosphere::Atmosphere;
use crate::app::engine::renderer::world::light::{DirectionalLight, sun_direction};
use crate::app::engine::renderer::world::stars::{Stars, world_to_sky};
use crate::app::engine::renderer::world::terrain::Terrain;
use crate::app::engine::renderer::world::water::Water;
use crate::app::{
    engine::{
        renderer::{
            camera::Camera,
            descriptors::DescriptorHandler,
            render_resources::{MaterialBuffer, RenderObject, RenderResourceManager, WorldMaps},
        },
        rendering_context::RenderingContext,
        timer::Timer,
    },
    logger::Logger,
};
use crate::config::{Anisotropy, EngineConfig};
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
    /// La mer, créée avec le terrain (même `sea_level`).
    pub(crate) water: Option<Water>,
    /// Cartes du monde pour les shaders (set 3), créées avec le terrain.
    pub(crate) world_maps: Option<WorldMaps>,
    /// Table des matériaux pour les shaders (set 4), créée avec le terrain.
    pub(crate) materials: Option<MaterialBuffer>,
    /// Pour créer le set des cartes du monde au moment du `setup`.
    descriptor_handler: Arc<DescriptorHandler>,
    /// Contexte GPU, conservé pour pouvoir allouer les buffers du terrain au
    /// moment du `setup` (la génération est pilotée par la scène) comme en cours de jeu.
    context: Arc<RenderingContext>,
    /// Nombre de frames que le renderer garde en vol — le terrain en a besoin pour dater
    /// la destruction différée de ses meshes.
    frames_in_flight: u64,
    game_clock: GameClock,
    /// Temps réel écoulé (s), pour les animations (scintillation, vagues) : elles ne
    /// suivent pas l'accélération du temps de jeu.
    pub(crate) real_time_secs: f32,
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
        let rrm = RenderResourceManager::new(
            context.clone(),
            logger.clone(),
            descriptor_handler.clone(),
            config.render.textures.anisotropy,
        )?;
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
            water: None,
            world_maps: None,
            materials: None,
            descriptor_handler,
            context,
            frames_in_flight: frames_in_flight as u64,
            game_clock: GameClock::new(&env.clock),
            real_time_secs: 0.0,
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

    /// Hot reload `Live` : l'UBO relit la config à chaque frame ; seules les copies faites
    /// à la création sont à mettre à jour, section par section pour ne pas écraser ce que
    /// la scène a réglé sur une section inchangée. À appeler AVANT de remplacer
    /// `self.config`, qui sert de référence pour la comparaison.
    pub(crate) fn hot_reload_live(&mut self, config: &EngineConfig) {
        let (old, new) = (&self.config.environment, &config.environment);
        if new.atmosphere != old.atmosphere {
            self.atmosphere.params = new.atmosphere;
        }
        if new.stars != old.stars {
            self.stars.params = new.stars;
        }
        if new.sun != old.sun {
            self.light.disk = new.sun;
        }
        if new.latitude_deg != old.latitude_deg {
            self.latitude = new.latitude_deg.to_radians();
        }
        if new.clock != old.clock {
            self.game_clock.hot_reload(&old.clock, &new.clock);
        }
        if new.water != old.water
            && let Some(water) = self.water.as_mut()
        {
            water.hot_reload(new.water);
        }

        let (old, new) = (&self.config.render, &config.render);
        // Boîte d'ombre active : celle du terrain s'il y en a un (cf. `generate_terrain`).
        let (old_box, new_box) = if self.terrain.is_some() {
            (&old.shadow.terrain, &new.shadow.terrain)
        } else {
            (&old.shadow.static_scene, &new.shadow.static_scene)
        };
        if new_box != old_box {
            self.light.shadow = new_box.clone();
        }
        if new.camera != old.camera {
            self.camera.hot_reload(&new.camera);
        }
        if let Some(terrain) = self.terrain.as_mut() {
            terrain.hot_reload(&config.debug);
        }
    }

    /// Hot reload `Sampler` : nouveau sampler des textures de surface, réécrit dans tous
    /// les sets qui le référencent (un par texture d'objet, celui des matériaux).
    pub(crate) fn hot_reload_sampler(&mut self, anisotropy: Anisotropy) -> Result<()> {
        let textures = &self.rrm.textures;
        let materials = &self.materials;
        let descriptor_handler = &self.descriptor_handler;
        self.rrm.texture_handler.hot_reload(anisotropy, |sampler| {
            for texture in textures.values() {
                if let Some(set) = texture.descriptor_set {
                    descriptor_handler.write_image_sampler(set, 0, texture.texture_image_view, sampler);
                }
            }
            if let Some(materials) = materials {
                materials.hot_reload_sampler(descriptor_handler, sampler);
            }
        })
    }

    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        profile!();
        // Pas de terrain ici : ses meshes arrivent au fil des frames (`update_terrain`).
        if let Some(water) = self.water.as_ref() {
            water.record_upload(command_buffer);
        }
        if let Some(world_maps) = self.world_maps.as_ref() {
            world_maps.record_upload(&self.rrm.texture_handler, command_buffer)?;
        }
        if let Some(materials) = self.materials.as_ref() {
            materials.record_upload(&self.rrm.texture_handler, command_buffer)?;
        }
        self.rrm.initialize(command_buffer)
    }

    /// Libère les stagings de `initialize`, une fois la passe de transfert exécutée.
    pub fn release_texture_staging(&mut self) {
        if let Some(world_maps) = self.world_maps.as_mut() {
            world_maps.release_texture_staging();
        }
        if let Some(materials) = self.materials.as_mut() {
            materials.release_texture_staging();
        }
        self.rrm.release_texture_staging();
    }

    pub fn update_world_data(&mut self, timer: &Timer, input_state: &InputState, aspect: f32) {
        profile!();
        self.game_clock.advance(timer.delta_secs() as f64);
        self.sync_sky();
        self.real_time_secs = timer.elapsed_secs();
        self.camera.update(input_state, timer, aspect);
        if let Some(terrain) = self.terrain.as_mut() {
            terrain.update_visibility(&self.camera, &self.light);
        }
        if let Some(water) = self.water.as_mut() {
            water.update(&self.camera);
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

    /// Copies staging → device des meshes de terrain fraîchement installés, et du masque
    /// des chunks installés s'il a changé.
    pub fn record_terrain_uploads(&mut self, command_buffer: vk::CommandBuffer) -> Result<()> {
        let Some(terrain) = self.terrain.as_mut() else {
            return Ok(());
        };
        terrain.record_uploads(command_buffer);
        if let (Some(texels), Some(world_maps)) =
            (terrain.take_loaded_mask(), self.world_maps.as_mut())
        {
            world_maps.record_loaded_chunks_upload(command_buffer, texels)?;
        }
        Ok(())
    }
}
