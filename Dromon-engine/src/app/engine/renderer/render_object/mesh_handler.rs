use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::renderer::render_object::mesh::Mesh;
use crate::app::engine::renderer::render_object::mesh::Vertex;
use crate::app::{engine::rendering_context::RenderingContext, logger::Logger};
use anyhow::{Context, Result};
use ash::vk;
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use std::path::Path;
use std::sync::Arc;

pub struct MeshHandler {
    pub context: Arc<RenderingContext>,
    pub logger: Arc<Logger>,
}

impl MeshHandler {
    pub fn new(context: Arc<RenderingContext>, logger: Arc<Logger>) -> MeshHandler {
        MeshHandler { context, logger }
    }

    pub fn create_model(&self, model_path: &str) -> Result<Mesh> {
        let (vertices, indices) = self.load_scene(model_path)?;
        let (vertex_staging_buffer, vertex_buffer) = Self::create_staging_and_device_buffer(
            self.context.clone(),
            vertices.as_slice(),
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;

        let (index_staging_buffer, index_buffer) = Self::create_staging_and_device_buffer(
            self.context.clone(),
            indices.as_slice(),
            vk::BufferUsageFlags::INDEX_BUFFER,
        )?;
        Ok(Mesh::new(
            self.context.clone(),
            vertices,
            vertex_staging_buffer,
            vertex_buffer,
            indices,
            index_staging_buffer,
            index_buffer,
        ))
    }

    /// Charge la scène glTF ENTIÈRE et la fusionne en une seule paire
    /// (vertices, indices).
    pub fn load_scene(&self, model_path: &str) -> Result<(Vec<Vertex>, Vec<u32>)> {
        let full_path = format!("{}{}", env!("CARGO_MANIFEST_DIR"), model_path);
        let full_path = Path::new(&full_path);

        // On évite gltf::import() qui chargerait AUSSI toutes les images
        // référencées par le .gltf.
        let gltf = gltf::Gltf::open(full_path)
            .with_context(|| format!("ouverture glTF {}", full_path.display()))?;
        let base = full_path.parent();
        let buffers = gltf::import_buffers(&gltf.document, base, gltf.blob)
            .context("chargement des buffers (scene.bin)")?;
        let document = gltf.document;

        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        // glTF est Y-up par convention ; notre moteur est Z-up (caméra up = +Z).
        let y_up_to_z_up = Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);

        let scene = document
            .scenes()
            .next()
            .context("le glTF n'a aucune scène")?;
        for node in scene.nodes() {
            Self::append_node(node, y_up_to_z_up, &buffers, &mut vertices, &mut indices);
        }

        // --- Recentrage + mise à l'échelle (étape de confort, retirable) ---
        Self::normalize(&mut vertices);

        // On applique le transform propre à cet Object (placement dans la scène).
        // Il vient APRÈS le recentrage : le modèle normalisé (~2 unités, centré
        // sur l'origine) est ensuite positionné/orienté/redimensionné par l'utilisateur.

        Ok((vertices, indices))
    }

    /// Parcourt récursivement un node et ses enfants en accumulant la transformation.
    /// `parent` est la matrice monde du parent ; `world = parent * locale`.
    fn append_node(
        node: gltf::Node,
        parent: Mat4,
        buffers: &[gltf::buffer::Data],
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
    ) {
        // transform().matrix() renvoie la matrice locale en column-major,
        // exactement le layout attendu par glam.
        let local = Mat4::from_cols_array_2d(&node.transform().matrix());
        let world = parent * local;

        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

                let Some(positions) = reader.read_positions() else {
                    continue; // primitive sans géométrie : on ignore
                };
                let positions: Vec<[f32; 3]> = positions.collect();

                // Les UV peuvent manquer sur certaines primitives : on retombe sur (0,0).
                let tex_coords: Vec<[f32; 2]> = reader
                    .read_tex_coords(0)
                    .map(|tc| tc.into_f32().collect())
                    .unwrap_or_default();

                // base = nombre de vertices déjà accumulés. Les indices de CETTE
                // primitive sont relatifs à ses propres vertices : on les décale
                // pour qu'ils pointent vers la bonne zone du Vec global.
                let base = vertices.len() as u32;

                for (i, pos) in positions.iter().enumerate() {
                    // transform_point3 applique rotation + échelle + translation.
                    let world_pos = world.transform_point3(Vec3::from(*pos));
                    let uv = tex_coords.get(i).copied().unwrap_or([0.0, 0.0]);
                    vertices.push(Vertex {
                        pos: world_pos,
                        color: Vec3::ONE,
                        texCoord: Vec2::from(uv),
                    });
                }

                match reader.read_indices() {
                    Some(read) => {
                        for idx in read.into_u32() {
                            indices.push(base + idx);
                        }
                    }
                    // Primitive non indexée : les vertices se suivent en triangles.
                    None => {
                        for i in 0..positions.len() as u32 {
                            indices.push(base + i);
                        }
                    }
                }
            }
        }

        for child in node.children() {
            Self::append_node(child, world, buffers, vertices, indices);
        }
    }

    /// Recentre le maillage sur l'origine et le met à l'échelle pour que sa plus
    /// grande dimension fasse ~2 unités. Modifie les positions en place.
    fn normalize(vertices: &mut [Vertex]) {
        if vertices.is_empty() {
            return;
        }

        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for v in vertices.iter() {
            min = min.min(v.pos);
            max = max.max(v.pos);
        }

        let center = (min + max) * 0.5;
        let extent = max - min;
        let largest = extent.x.max(extent.y).max(extent.z);
        let scale = if largest > 0.0 { 2.0 / largest } else { 1.0 };

        for v in vertices.iter_mut() {
            v.pos = (v.pos - center) * scale;
        }
    }

    pub fn create_staging_and_device_buffer<T: Copy>(
        context: Arc<RenderingContext>,
        data: &[T],
        role: vk::BufferUsageFlags,
    ) -> Result<(Buffer, Buffer)> {
        let staging_buffer = Buffer::new(
            context.clone(),
            std::mem::size_of_val(data) as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        staging_buffer.map_and_unmap(data)?;

        let device_buffer = Buffer::new(
            context.clone(),
            std::mem::size_of_val(data) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | role,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;

        Ok((staging_buffer, device_buffer))
    }

    pub fn initialize(&self, mesh: &Mesh, command_buffer: &vk::CommandBuffer) {
        unsafe {
            self.context.device.cmd_copy_buffer(
                *command_buffer,
                mesh.vertex_staging_buffer.buffer,
                mesh.vertex_buffer.buffer,
                &[vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: std::mem::size_of_val(mesh.vertices.as_slice()) as u64,
                }],
            );

            self.context.device.cmd_copy_buffer(
                *command_buffer,
                mesh.index_staging_buffer.buffer,
                mesh.index_buffer.buffer,
                &[vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: std::mem::size_of_val(mesh.indices.as_slice()) as u64,
                }],
            );
        }
    }
}
