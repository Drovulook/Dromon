use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::renderer::render_resources::object_mesh::ObjectMesh;
use crate::app::engine::renderer::render_resources::object_mesh::ObjectVertex;
use crate::app::{engine::rendering_context::RenderingContext, logger::Logger};
use anyhow::{Context, Result};
use ash::vk;
use glam::{Mat3, Mat4, Vec2, Vec3};
use std::path::Path;
use std::sync::Arc;

pub struct ObjectMeshHandler {
    pub context: Arc<RenderingContext>,
    pub logger: Arc<Logger>,
}

impl ObjectMeshHandler {
    pub fn new(context: Arc<RenderingContext>, logger: Arc<Logger>) -> ObjectMeshHandler {
        ObjectMeshHandler { context, logger }
    }

    pub fn create_model(&self, model_path: &str, smooth: bool) -> Result<ObjectMesh> {
        let (vertices, indices) = self.load_scene(model_path, smooth)?;
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
        Ok(ObjectMesh::new(
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
    pub fn load_scene(
        &self,
        model_path: &str,
        smooth: bool,
    ) -> Result<(Vec<ObjectVertex>, Vec<u32>)> {
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

        let mut vertices: Vec<ObjectVertex> = Vec::new();
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

        // Lissage optionnel : le modèle est exporté en flat shading (une normale
        // par facette, vertices dupliqués aux coutures). On recalcule des normales
        // LISSES en moyennant celles qui partagent une même position.
        if smooth {
            Self::smooth_normals(&mut vertices);
        }

        // --- Recentrage + mise à l'échelle (étape de confort, retirable) ---
        Self::normalize(&mut vertices);

        Ok((vertices, indices))
    }

    /// Parcourt récursivement un node et ses enfants en accumulant la transformation.
    /// `parent` est la matrice monde du parent ; `world = parent * locale`.
    fn append_node(
        node: gltf::Node,
        parent: Mat4,
        buffers: &[gltf::buffer::Data],
        vertices: &mut Vec<ObjectVertex>,
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

                // Normales par vertex (attribut NORMAL du .bin). Peut manquer sur
                // certaines primitives : on retombe alors sur +Z (faute de mieux).
                let normals: Vec<[f32; 3]> = reader
                    .read_normals()
                    .map(|n| n.collect())
                    .unwrap_or_default();

                // Une normale ne se transforme pas comme un point : il faut la
                // matrice normale = inverse-transposée de la partie 3x3 de world.
                let normal_matrix = Mat3::from_mat4(world).inverse().transpose();

                // base = nombre de vertices déjà accumulés. Les indices de CETTE
                // primitive sont relatifs à ses propres vertices : on les décale
                let base = vertices.len() as u32;

                for (i, pos) in positions.iter().enumerate() {
                    // transform_point3 applique rotation + échelle + translation.
                    let world_pos = world.transform_point3(Vec3::from(*pos));
                    let uv = tex_coords.get(i).copied().unwrap_or([0.0, 0.0]);
                    let normal = normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]);
                    // On renormalise : la matrice normale ne préserve pas la longueur.
                    let world_normal = (normal_matrix * Vec3::from(normal)).normalize();
                    vertices.push(ObjectVertex {
                        pos: world_pos,
                        color: Vec3::ONE,
                        texCoord: Vec2::from(uv),
                        normal: world_normal,
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

    fn smooth_normals(vertices: &mut [ObjectVertex]) {
        use std::collections::HashMap;

        const QUANT: f32 = 1.0e4;
        let key = |p: Vec3| -> [i32; 3] {
            [
                (p.x * QUANT).round() as i32,
                (p.y * QUANT).round() as i32,
                (p.z * QUANT).round() as i32,
            ]
        };

        // Somme des normales par position partagée.
        let mut sums: HashMap<[i32; 3], Vec3> = HashMap::new();
        for v in vertices.iter() {
            *sums.entry(key(v.pos)).or_insert(Vec3::ZERO) += v.normal;
        }

        // Réassigne à chaque vertex la normale moyenne (renormalisée) de sa
        // position.
        for v in vertices.iter_mut() {
            if let Some(&sum) = sums.get(&key(v.pos)) {
                v.normal = sum.normalize_or_zero();
            }
        }
    }

    /// Recentre le maillage sur l'origine et le met à l'échelle pour que sa plus
    /// grande dimension fasse ~2 unités. Modifie les positions en place.
    fn normalize(vertices: &mut [ObjectVertex]) {
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

    pub fn initialize(&self, mesh: &ObjectMesh, command_buffer: &vk::CommandBuffer) {
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
