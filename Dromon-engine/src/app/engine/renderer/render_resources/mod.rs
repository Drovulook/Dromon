mod material_buffer;
mod object_mesh;
mod object_mesh_handler;
mod terrain_mesh;
mod terrain_textures;
// Visible du renderer : texgen y range ses storage images.
pub(super) mod texture;
mod texture_handler;
mod transform;
mod water_mesh;
mod world_maps;

use crate::app::{
    engine::{
        renderer::renderer_initialize::submit_one_time,
        renderer::{
            buffer::Buffer,
            descriptors::DescriptorHandler,
            render_resources::{
                object_mesh_handler::ObjectMeshHandler, texture_handler::TextureHandler,
            },
        },
        rendering_context::RenderingContext,
    },
    logger::Logger,
};
use crate::config::{Anisotropy, ModelSource};
use anyhow::Result;
use ash::vk;
pub use material_buffer::MaterialBuffer;
pub use object_mesh::{ObjectMesh, ObjectVertex};
use std::{collections::HashMap, sync::Arc};
pub use terrain_mesh::{
    MeshData, NoRoom, SEAM_VERTEX_FIXED, SEAM_VERTEX_SLAB_HIGH, TerrainMesh, TerrainVertex,
};
use texture::Texture;
pub use transform::Transform;
pub use water_mesh::{WATER_GRID_RESOLUTION, WaterMesh, WaterVertex};
pub use world_maps::WorldMaps;

pub struct RenderResourceManager {
    logger: Arc<Logger>,
    pub textures: HashMap<String, Arc<Texture>>,
    /// Textures en attente d'envoi et leur staging, libéré par `release_texture_staging`.
    pending_textures: Vec<(Arc<Texture>, Buffer)>,
    pub meshes: HashMap<String, Arc<ObjectMesh>>,
    /// Meshes pas encore copiés vers le device, vidé par `release_texture_staging`.
    pending_meshes: Vec<Arc<ObjectMesh>>,
    pub texture_handler: TextureHandler,
    pub mesh_handler: ObjectMeshHandler,
}

impl RenderResourceManager {
    pub fn new(
        context: Arc<RenderingContext>,
        logger: Arc<Logger>,
        descriptor_handler: Arc<DescriptorHandler>,
        anisotropy: Anisotropy,
    ) -> Result<RenderResourceManager> {
        Ok(RenderResourceManager {
            logger: logger.clone(),
            textures: HashMap::new(),
            pending_textures: Vec::new(),
            meshes: HashMap::new(),
            pending_meshes: Vec::new(),
            texture_handler: TextureHandler::new(
                context.clone(),
                logger.clone(),
                descriptor_handler,
                anisotropy,
            )?,
            mesh_handler: ObjectMeshHandler::new(context, logger),
        })
    }

    pub fn add_texture(&mut self, name: String, path: String) -> Result<()> {
        if self.textures.contains_key(&name) {
            self.logger
                .warn(&format!("texture {} already exists", name));
            return Ok(());
        }
        let (texture, staging) = self.texture_handler.create_object_texture(&path)?;
        let texture = Arc::new(texture);
        self.pending_textures.push((texture.clone(), staging));
        self.textures.insert(name, texture);
        Ok(())
    }

    /// Texture construite en mémoire : `pixels` en RGBA8 sRGB, ligne par ligne.
    pub fn add_texture_from_data(
        &mut self,
        name: String,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<()> {
        if self.textures.contains_key(&name) {
            self.logger
                .warn(&format!("texture {} already exists", name));
            return Ok(());
        }
        let (texture, staging) = self
            .texture_handler
            .create_object_texture_from_rgba(width, height, pixels)?;
        let texture = Arc::new(texture);
        self.pending_textures.push((texture.clone(), staging));
        self.textures.insert(name, texture);
        Ok(())
    }

    pub fn add_mesh(&mut self, name: String, source: &ModelSource, smooth: bool) -> Result<()> {
        if self.meshes.contains_key(&name) {
            self.logger.warn(&format!("mesh {} already exists", name));
            return Ok(());
        }
        let model = Arc::new(self.mesh_handler.create_model(&source.resolve()?, smooth)?);
        self.pending_meshes.push(model.clone());
        self.meshes.insert(name, model);
        Ok(())
    }

    /// Mesh construit en mémoire (ex. sol du lookdev), envoyé au GPU comme les autres.
    pub fn add_mesh_from_data(
        &mut self,
        name: String,
        vertices: Vec<ObjectVertex>,
        indices: Vec<u32>,
    ) -> Result<()> {
        if self.meshes.contains_key(&name) {
            self.logger.warn(&format!("mesh {} already exists", name));
            return Ok(());
        }
        let mesh = Arc::new(self.mesh_handler.create_mesh_from_data(vertices, indices)?);
        self.pending_meshes.push(mesh.clone());
        self.meshes.insert(name, mesh);
        Ok(())
    }

    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        for (texture, staging) in &self.pending_textures {
            self.texture_handler
                .initialize(texture, staging, command_buffer)?;
        }
        for mesh in &self.pending_meshes {
            self.mesh_handler.initialize(mesh, command_buffer);
        }
        Ok(())
    }

    /// Libère les stagings des textures et oublie les meshes envoyés. Seulement une fois
    /// la passe de transfert **exécutée** (après l'attente de la queue).
    pub fn release_pending(&mut self) {
        self.pending_textures.clear();
        self.pending_meshes.clear();
    }

    /// Envoie tout de suite les meshes et textures en attente, hors des frames : ajouts
    /// en cours de jeu. Bloquant (quelques ms).
    pub fn upload_pending(&mut self) -> Result<()> {
        if self.pending_meshes.is_empty() && self.pending_textures.is_empty() {
            return Ok(());
        }
        let context = self.mesh_handler.context.clone();
        submit_one_time(&context, |command_buffer| self.initialize(&command_buffer))?;
        self.release_pending();
        Ok(())
    }

    pub fn get_texture(&self, name: &str) -> Option<Arc<Texture>> {
        self.textures.get(name).cloned()
    }

    pub fn get_mesh(&self, name: &str) -> Option<Arc<ObjectMesh>> {
        self.meshes.get(name).cloned()
    }
}

pub struct RenderObject {
    pub mesh: Arc<ObjectMesh>,
    pub texture: Arc<Texture>,
    pub transform: Transform,
}

impl RenderObject {
    pub fn new(mesh: Arc<ObjectMesh>, texture: Arc<Texture>, transform: Transform) -> RenderObject {
        RenderObject {
            mesh,
            texture,
            transform,
        }
    }
}
