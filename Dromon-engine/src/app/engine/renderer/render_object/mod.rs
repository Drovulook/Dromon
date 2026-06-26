mod mesh;
mod mesh_handler;
mod texture;
mod texture_handler;
mod transform;

use crate::app::{
    engine::{
        renderer::{
            descriptors::DescriptorHandler,
            render_object::{mesh_handler::MeshHandler, texture_handler::TextureHandler},
        },
        rendering_context::RenderingContext,
    },
    logger::Logger,
};
use anyhow::Result;
use ash::vk;
use mesh::Mesh;
pub use mesh::Vertex;
use std::{collections::HashMap, sync::Arc};
use texture::Texture;
pub use transform::Transform;

pub struct RenderObjectResourceManager {
    context: Arc<RenderingContext>,
    logger: Arc<Logger>,
    pub textures: HashMap<String, Arc<Texture>>,
    pub meshes: HashMap<String, Arc<Mesh>>,
    pub texture_handler: TextureHandler,
    pub mesh_handler: MeshHandler,
}

impl RenderObjectResourceManager {
    pub fn new(
        context: Arc<RenderingContext>,
        logger: Arc<Logger>,
        descriptor_handler: Arc<DescriptorHandler>,
    ) -> Result<RenderObjectResourceManager> {
        Ok(RenderObjectResourceManager {
            context: context.clone(),
            logger: logger.clone(),
            textures: HashMap::new(),
            meshes: HashMap::new(),
            texture_handler: TextureHandler::new(
                context.clone(),
                logger.clone(),
                descriptor_handler,
            )?,
            mesh_handler: MeshHandler::new(context, logger),
        })
    }

    pub fn add_texture(&mut self, name: String, path: String) -> Result<()> {
        if self.textures.contains_key(&name) {
            self.logger
                .warn(&format!("texture {} already exists", name));
            return Ok(());
        }
        let texture = self.texture_handler.create_texture(&path)?;
        self.textures.insert(name, Arc::new(texture));
        Ok(())
    }

    pub fn add_mesh(&mut self, name: String, path: String, smooth: bool) -> Result<()> {
        if self.meshes.contains_key(&name) {
            self.logger.warn(&format!("mesh {} already exists", name));
            return Ok(());
        }
        let model = self.mesh_handler.create_model(&path, smooth)?;
        self.meshes.insert(name, Arc::new(model));
        Ok(())
    }

    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        for texture in self.textures.values() {
            self.texture_handler.initialize(texture, command_buffer)?;
        }
        for mesh in self.meshes.values() {
            self.mesh_handler.initialize(mesh, command_buffer);
        }
        Ok(())
    }

    pub fn get_texture(&self, name: &str) -> Option<Arc<Texture>> {
        self.textures.get(name).cloned()
    }

    pub fn get_mesh(&self, name: &str) -> Option<Arc<Mesh>> {
        self.meshes.get(name).cloned()
    }
}

pub struct RenderObject {
    pub mesh: Arc<Mesh>,
    pub texture: Arc<Texture>,
    pub transform: Transform,
}

impl RenderObject {
    pub fn new(mesh: Arc<Mesh>, texture: Arc<Texture>, transform: Transform) -> RenderObject {
        RenderObject {
            mesh,
            texture,
            transform,
        }
    }
}
