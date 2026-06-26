use crate::app::engine::inputs::InputState;
use crate::app::engine::renderer::camera::Camera;
use anyhow::{Context, Result};
use ash::vk;
use std::sync::Arc;

use crate::app::{
    engine::{
        renderer::{
            descriptors::DescriptorHandler,
            render_object::{RenderObject, RenderObjectResourceManager, Transform},
        },
        rendering_context::RenderingContext,
        timer::Timer,
    },
    logger::Logger,
};

const GLTF_PATH: &str = "/res/models/dromon_ship/scene.gltf";
const TEXTURE_PATH: &str = "/res/models/dromon_ship/DefaultMaterial_baseColor.png";

pub struct DirectionalLight {
    pub direction: glam::Vec3,
    pub color: glam::Vec3,
    pub intensity: f32,
}

pub struct World {
    pub logger: Arc<Logger>,
    pub rorm: RenderObjectResourceManager,
    pub render_objects: Vec<RenderObject>,
    pub camera: Camera,
    pub light: DirectionalLight,
}

impl World {
    pub fn new(
        logger: Arc<Logger>,
        context: Arc<RenderingContext>,
        descriptor_handler: Arc<DescriptorHandler>,
    ) -> Result<World> {
        let mut rorm =
            RenderObjectResourceManager::new(context, logger.clone(), descriptor_handler)?;
        rorm.add_texture("texture1".to_string(), TEXTURE_PATH.to_string())?;
        rorm.add_texture("texture2".to_string(), TEXTURE_PATH.to_string())?;
        rorm.add_mesh("mesh1".to_string(), GLTF_PATH.to_string(), true)?;
        rorm.add_mesh("mesh2".to_string(), GLTF_PATH.to_string(), true)?;

        let render_objects = vec![
            RenderObject::new(
                rorm.get_mesh("mesh1")
                    .context("mesh \"mesh1\" introuvable")?,
                rorm.get_texture("texture1")
                    .context("texture \"texture1\" introuvable")?,
                Transform {
                    translation: glam::Vec3::new(0.0, 1.0, 0.0),
                    ..Default::default()
                },
            ),
            RenderObject::new(
                rorm.get_mesh("mesh2")
                    .context("mesh \"mesh2\" introuvable")?,
                rorm.get_texture("texture2")
                    .context("texture \"texture2\" introuvable")?,
                Transform {
                    translation: glam::Vec3::new(0.0, -1.0, 0.0),
                    ..Default::default()
                },
            ),
        ];

        Ok(World {
            logger,
            rorm,
            render_objects,
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

    pub fn update_render_objects(&mut self, timer: &Timer) {
        for render_object in &mut self.render_objects {
            render_object.transform.rotation.z += timer.delta_secs();
        }
    }
}
