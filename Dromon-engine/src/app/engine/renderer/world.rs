use crate::app::engine::inputs::InputState;
use crate::app::engine::renderer::camera::Camera;
use crate::profile;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use crate::app::{
    engine::{
        renderer::{
            descriptors::DescriptorHandler,
            render_resources::{RenderObject, RenderResourceManager, TerrainMesh},
        },
        rendering_context::RenderingContext,
        terrain_generation::{ChunkManager, GenParams, mesh_chunk},
        timer::Timer,
    },
    logger::Logger,
};
use glam::IVec2;

// Paramètres du « frustum » orthographique de la lumière (la boîte qui doit
// englober toute la scène projetant des ombres). La boîte SUIT la caméra
// (cf. `view_proj`) ; ces valeurs fixent sa taille, pas sa position. Monde Z-up.
//
// Compromis résolution : la shadow map (2048²) est étalée sur `2*HALF_SIZE`
// unités → ~`2*HALF_SIZE / 2048` u/texel. Plus la boîte est grande, plus on
// couvre de terrain mais plus les ombres deviennent grossières. ~150 ⇒ ~0.15
// u/texel, correct à l'échelle voxel.
/// Réglages du « frustum » orthographique de la shadow map. Portés par la scène
/// (via [`DirectionalLight`]) car l'échelle dépend du contenu : une petite scène
/// d'objets fixes et un terrain géant qu'on survole ne veulent ni la même taille
/// de boîte, ni la même stratégie de centrage.
///
/// La shadow map (2048²) est étalée sur `2 * half_size` unités → résolution
/// `2 * half_size / 2048` u/texel : plus la boîte est grande, plus on couvre de
/// monde mais plus les ombres deviennent grossières.
pub struct ShadowConfig {
    /// Demi-largeur/hauteur de la boîte orthographique, en unités monde.
    pub half_size: f32,
    /// Plans near/far le long de l'axe lumière, depuis l'« œil » virtuel.
    /// ⚠ `far` élargit aussi le biais anti-acné (exprimé en profondeur
    /// normalisée) : un `far` énorme sur de petits objets décolle leur ombre.
    pub near: f32,
    pub far: f32,
    /// Recul de l'« œil » virtuel le long de `-direction`.
    pub eye_distance: f32,
    /// Si `true`, la boîte suit la caméra (indispensable pour un grand terrain) ;
    /// sinon elle reste centrée sur l'origine (idéal pour une scène d'objets
    /// fixes autour de l'origine).
    pub follow_camera: bool,
    /// Quand `follow_camera`, distance devant la caméra (le long du regard) du
    /// point de centrage : on dépense le budget d'ombre là où le joueur regarde.
    pub focus_distance: f32,
}

impl Default for ShadowConfig {
    /// Défauts pour une **petite scène statique** centrée sur l'origine
    /// (p. ex. model_sandbox). `generate_terrain` les remplace par des valeurs
    /// adaptées au terrain.
    fn default() -> Self {
        ShadowConfig {
            half_size: 15.0,
            near: 0.1,
            far: 60.0,
            eye_distance: 30.0,
            follow_camera: false,
            focus_distance: 0.0,
        }
    }
}

pub struct DirectionalLight {
    pub direction: glam::Vec3,
    pub color: glam::Vec3,
    pub intensity: f32,
    pub shadow: ShadowConfig,
}

impl DirectionalLight {
    /// Matrice view*proj de la lumière : on place une caméra orthographique le
    /// long de la direction du soleil, regardant le centre de la boîte d'ombre.
    /// C'est l'équivalent de `camera.view * camera.proj`, mais pour la lumière,
    /// et en projection orthographique (rayons parallèles = pas de perspective).
    ///
    /// Le centre dépend de [`ShadowConfig::follow_camera`] : soit l'origine
    /// (scène statique), soit un point devant la caméra (terrain) pour que la
    /// zone ombrée suive le joueur — `cam_pos`/`cam_front` servent à ce calcul.
    ///
    /// Note : volontairement PAS de flip de l'axe Y (contrairement à la caméra).
    /// La shadow map est rasterisée ET échantillonnée avec cette même matrice,
    /// donc le résultat reste cohérent ; inverser Y ici ne ferait que retourner
    /// la texture sans rien changer au calcul d'ombre.
    pub fn view_proj(&self, cam_pos: glam::Vec3, cam_front: glam::Vec3) -> glam::Mat4 {
        profile!();
        let dir = self.direction.normalize();
        let s = &self.shadow;

        let center = if s.follow_camera {
            cam_pos + cam_front * s.focus_distance
        } else {
            glam::Vec3::ZERO
        };
        let eye = center - dir * s.eye_distance;

        // « up » de la caméra-lumière : Z-up en général, sauf si la lumière est
        // quasi verticale (colinéaire à Z) — on bascule alors sur Y pour éviter
        // un look_at dégénéré.
        let up = if dir.cross(glam::Vec3::Z).length_squared() < 1e-4 {
            glam::Vec3::Y
        } else {
            glam::Vec3::Z
        };

        let view = glam::Mat4::look_at_rh(eye, center, up);
        // orthographic_rh (et non _gl) : profondeur clippée dans [0, 1], la
        // convention attendue par Vulkan.
        let proj = glam::Mat4::orthographic_rh(
            -s.half_size,
            s.half_size,
            -s.half_size,
            s.half_size,
            s.near,
            s.far,
        );
        proj * view
    }
}

pub struct World {
    pub logger: Arc<Logger>,
    pub rrm: RenderResourceManager,
    pub render_objects: Vec<RenderObject>,
    pub camera: Camera,
    pub light: DirectionalLight,
    /// Données du terrain (voxels). `None` tant que la scène n'a pas appelé
    /// [`World::generate_terrain`]. Conservé pour l'édition future (creuser,
    /// poser des minerais, etc.).
    pub chunk_manager: Option<ChunkManager>,
    /// Un mesh GPU par chunk de terrain.
    pub terrain_meshes: Vec<TerrainMesh>,
    /// Contexte GPU, conservé pour pouvoir allouer les buffers du terrain au
    /// moment du `setup` (la génération est pilotée par la scène).
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
            terrain_meshes: Vec::new(),
            context,
        })
    }

    /// Génère une grille de `chunks_x × chunks_y` chunks de terrain, centrée sur
    /// l'origine (en coordonnées chunk), et construit un `TerrainMesh` par chunk.
    /// Appelée par la scène dans `setup` ; l'upload GPU des meshes se fait ensuite
    /// dans [`World::initialize`].
    pub fn generate_terrain(
        &mut self,
        params: GenParams,
        chunks_x: u32,
        chunks_y: u32,
    ) -> Result<()> {
        profile!();
        // Le terrain s'étend sur tout le monde et on le survole : la boîte d'ombre
        // doit être grande et suivre la caméra (cf. ShadowConfig). On garde une
        // résolution correcte (2*150/2048 ≈ 0.15 u/texel) ; CSM plus tard pour les
        // ombres nettes à toutes distances.
        self.light.shadow = ShadowConfig {
            half_size: 150.0,
            near: 1.0,
            far: 600.0,
            eye_distance: 300.0,
            follow_camera: true,
            focus_distance: 90.0,
        };

        let mut manager = ChunkManager::new(params);

        // Coordonnées des chunks, centrées sur l'origine.
        let half_x = chunks_x as i32 / 2;
        let half_y = chunks_y as i32 / 2;
        let mut coords = Vec::with_capacity((chunks_x * chunks_y) as usize);
        for cx in 0..chunks_x as i32 {
            for cy in 0..chunks_y as i32 {
                coords.push(IVec2::new(cx - half_x, cy - half_y));
            }
        }

        // 1) Génère TOUS les chunks (données voxel) d'abord, pour que le meshing
        //    puisse échantillonner les chunks voisins aux bords (coutures continues).
        for &coord in &coords {
            manager.generate_chunk(coord);
        }

        // 2) Meshing : un TerrainMesh par chunk. On saute les chunks **vides**
        //    (aucune surface dans la zone maillée, p. ex. relief entièrement
        //    sous z=0) : un buffer de taille 0 est interdit par Vulkan.
        let mut terrain_meshes = Vec::with_capacity(coords.len());
        {
            profile!("create terrain meshes from chunks");
            for &coord in &coords {
                let (vertices, indices) = mesh_chunk(&manager, coord);
                if vertices.is_empty() || indices.is_empty() {
                    continue;
                }
                terrain_meshes.push(TerrainMesh::new(self.context.clone(), vertices, indices)?);
            }
        }

        self.chunk_manager = Some(manager);
        self.terrain_meshes = terrain_meshes;
        Ok(())
    }

    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        profile!();
        self.rrm.initialize(command_buffer)?;
        for mesh in &self.terrain_meshes {
            mesh.initialize(command_buffer);
        }
        Ok(())
    }

    pub fn update_world_data(&mut self, timer: &Timer, input_state: &InputState, aspect: f32) {
        profile!();
        self.camera.update(input_state, timer, aspect);
    }
}
