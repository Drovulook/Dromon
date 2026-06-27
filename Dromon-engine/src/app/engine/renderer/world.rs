use crate::app::engine::inputs::InputState;
use crate::app::engine::renderer::camera::Camera;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use crate::app::{
    engine::{
        renderer::{
            descriptors::DescriptorHandler,
            render_object::{RenderObject, RenderObjectResourceManager},
        },
        rendering_context::RenderingContext,
        timer::Timer,
    },
    logger::Logger,
};

// Paramètres du « frustum » orthographique de la lumière (la boîte qui doit
// englober toute la scène projetant des ombres). À élargir si des objets sortent
// de la zone ombrée. Monde en Z-up.
const SHADOW_ORTHO_HALF_SIZE: f32 = 15.0; // demi-largeur/hauteur de la boîte
const SHADOW_NEAR: f32 = 0.1;
const SHADOW_FAR: f32 = 60.0;
const SHADOW_EYE_DISTANCE: f32 = 30.0; // recul de l'« œil » virtuel le long de -direction

pub struct DirectionalLight {
    pub direction: glam::Vec3,
    pub color: glam::Vec3,
    pub intensity: f32,
}

impl DirectionalLight {
    /// Matrice view*proj de la lumière : on place une caméra orthographique le
    /// long de la direction du soleil, regardant le centre de la scène. C'est
    /// l'équivalent de `camera.view * camera.proj`, mais pour la lumière, et en
    /// projection orthographique (rayons parallèles = pas de perspective).
    ///
    /// Note : volontairement PAS de flip de l'axe Y (contrairement à la caméra).
    /// La shadow map est rasterisée ET échantillonnée avec cette même matrice,
    /// donc le résultat reste cohérent ; inverser Y ici ne ferait que retourner
    /// la texture sans rien changer au calcul d'ombre.
    pub fn view_proj(&self) -> glam::Mat4 {
        let dir = self.direction.normalize();
        let center = glam::Vec3::ZERO;
        let eye = center - dir * SHADOW_EYE_DISTANCE;

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
            -SHADOW_ORTHO_HALF_SIZE,
            SHADOW_ORTHO_HALF_SIZE,
            -SHADOW_ORTHO_HALF_SIZE,
            SHADOW_ORTHO_HALF_SIZE,
            SHADOW_NEAR,
            SHADOW_FAR,
        );
        proj * view
    }
}

pub struct World {
    pub logger: Arc<Logger>,
    pub rorm: RenderObjectResourceManager,
    pub render_objects: Vec<RenderObject>,
    pub camera: Camera,
    pub light: DirectionalLight,
}

impl World {
    /// Crée un monde *vide* : seul le gestionnaire de ressources (`rorm`) et des
    /// valeurs par défaut (caméra, lumière) sont initialisés. Le contenu concret
    /// (assets + `RenderObject`) est fourni par la `Scene` via `Scene::setup`, qui
    /// est appelée juste après la construction du `Renderer`. La scène peut aussi
    /// modifier `world.light` à ce moment-là.
    pub fn new(
        logger: Arc<Logger>,
        context: Arc<RenderingContext>,
        descriptor_handler: Arc<DescriptorHandler>,
    ) -> Result<World> {
        let rorm = RenderObjectResourceManager::new(context, logger.clone(), descriptor_handler)?;

        Ok(World {
            logger,
            rorm,
            render_objects: Vec::new(),
            camera: Camera::default(),
            light: DirectionalLight {
                direction: glam::Vec3::new(-0.3, -0.5, -1.0),
                color: glam::Vec3::ONE,
                intensity: 1.0,
            },
        })
    }

    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        self.rorm.initialize(command_buffer)?;
        Ok(())
    }

    pub fn update_world_data(&mut self, timer: &Timer, input_state: &InputState, aspect: f32) {
        self.camera.update(input_state, timer, aspect);
    }
}
