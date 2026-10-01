use super::buffer::Buffer;
use crate::app::engine::renderer::world::atmosphere::Atmosphere;
use crate::app::engine::renderer::world::light::DirectionalLight;
use crate::app::engine::{renderer::camera::Camera, rendering_context::RenderingContext};
use crate::profile;
use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use std::sync::Arc;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct UniformBufferObject {
    view: Mat4,
    proj: Mat4,
    camera_position: Vec4, // w inutilisé (pour l'instant)
    // Matrice « view*proj » de la lumière (projection orthographique depuis le
    // soleil).
    light_view_proj: Mat4,
    // des uniform buffers Vulkan : un vec3 est aligné sur 16 octets mais n'en
    // occupe que 12
    light_direction: Vec4, // xyz = direction de propagation, w inutilisé
    light_color: Vec4,
    fog: Vec4,           // xyz = couleur du ciel, w =  σ₀
    fog_params: Vec4,    // x = H, y = fog_anisotropy, z = halo_strength, w inutilisé
    shadow_params: Vec4, // x = épaisseur de la boîte d'ombre, y = texel (unités monde)
    sun_disk: Vec4,      // x = rayon angulaire (rad), y = edge_softness, z = intensité, w inutilisé
}

pub struct UniformBuffer {
    context: Arc<RenderingContext>,
    buffer: Buffer,
    mapped: *mut u8,
}

impl UniformBuffer {
    pub fn new(context: Arc<RenderingContext>) -> Result<Self> {
        let size = std::mem::size_of::<UniformBufferObject>() as vk::DeviceSize;
        let buffer = Buffer::new(
            context.clone(),
            size,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        let mapped = buffer.map()?;
        Ok(Self {
            context,
            buffer,
            mapped,
        })
    }

    pub fn get_handle(&self) -> vk::Buffer {
        self.buffer.buffer
    }

    pub fn update(&self, camera: &Camera, sun: &DirectionalLight, atmosphere: &Atmosphere) {
        profile!();
        let ubo = UniformBufferObject {
            view: camera.view,
            proj: camera.proj,
            camera_position: camera.position.extend(1.0),
            light_view_proj: sun.view_proj(camera.position, camera.front()),
            light_direction: sun.direction.extend(0.0),
            light_color: sun.color.extend(sun.intensity),
            fog: atmosphere.sky_color.extend(atmosphere.fog_density),
            shadow_params: Vec4::new(sun.shadow.depth_range(), sun.shadow.texel_size(), 0.0, 0.0),
            fog_params: Vec4::new(
                atmosphere.fog_scale_height,
                atmosphere.fog_anisotropy,
                atmosphere.halo_strength,
                0.0,
            ),
            sun_disk: Vec4::new(
                sun.disk.angular_radius,
                sun.disk.edge_softness,
                sun.disk.intensity,
                0.0,
            ),
        };

        unsafe {
            std::ptr::copy_nonoverlapping(
                &ubo as *const UniformBufferObject as *const u8,
                self.mapped,
                std::mem::size_of::<UniformBufferObject>(),
            );
        }
    }
}
