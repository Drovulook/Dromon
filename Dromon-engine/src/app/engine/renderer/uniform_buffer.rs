use super::buffer::Buffer;
use crate::app::engine::renderer::world::atmosphere::Atmosphere;
use crate::app::engine::renderer::world::light::DirectionalLight;
use crate::app::engine::renderer::world::stars::Stars;
use crate::app::engine::renderer::world::water::{LAND_MARGIN, WAVE_COUNT, Water};
use crate::app::engine::{renderer::camera::Camera, rendering_context::RenderingContext};
use crate::config::EngineConfig;
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
    fog: Vec4,           // xyz = couleur de l'horizon, w =  σ₀
    fog_params: Vec4,    // x = H, y = fog_anisotropy, z = halo_strength, w inutilisé
    shadow_params: Vec4, // x = épaisseur de la boîte d'ombre, y = texel (unités monde), z = texel (uv), w = 1 si ombres actives
    shadow_bias: Vec4,   // x = normal_offset_texels, y = depth_bias_texels, zw inutilisés
    sun_disk: Vec4,      // x = rayon angulaire (rad), y = edge_softness, z = intensité, w inutilisé
    sky_zenith: Vec4,    // xyz = couleur du zénith, w = exposant du dégradé
    ambient: Vec4,       // xyz = lumière ambiante du ciel, w = ground_bounce
    // Rotation monde → ciel fixe (3x3 dans une Mat4 : un mat3 std140 a des colonnes
    // paddées à 16 octets, que glam::Mat3 n'a pas).
    sky_rotation: Mat4,
    stars: Vec4, // x = luminosité × visibilité, y = rayon (px), z = densité, w = scintillation
    world_maps: Vec4, // monde → UV : grille fine uv = (xy − x) · y, climat uv = (xy − z) · w
    water: Vec4, // x = sea_level, y = portée horizontale, z = marge terre, w = 1 si mer
    water_absorption: Vec4, // xyz = k par canal, w = 1 si caméra sous la mer
    water_deep_color: Vec4, // xyz = couleur de l'eau profonde chaude, w = variance des pentes (reflet)
    water_deep_cold: Vec4, // xyz = couleur de l'eau profonde froide, w inutilisé
    water_sst: Vec4,       // x = sst_cold, y = sst_warm (°C), zw inutilisés
    time: Vec4, // x = temps réel (s), yzw inutilisés
    // Houle : 2 vec4 par vague, (direction, k, A) puis (ω, phase, –, –).
    waves: [Vec4; 2 * WAVE_COUNT],
    wave_params: Vec4, // x = coast_fade, yzw inutilisés
    // Écume (`FoamParams`) : (albedo, zone_depth, patch_size, bubble_size),
    // (bubble_speed, drift, surge_period, surge_spread), (calm_zone, calm_density, –, –).
    foam: [Vec4; 3],
    material_tint: Vec4, // x = humidité côtière (ajout), y = sa portée (voxels), zw inutilisés
    tonemap: Vec4,       // x = 1 si actif, y = exposition, zw inutilisés
    // Textures du terrain (`MaterialTexturesConfig`).
    texture_blend: Vec4,  // x = triplanar_sharpness, y = projection_blend_depth, z = material_blend_depth, w inutilisé
    texture_detail: Vec4, // x = normal_strength, y = ao_strength, z = flat_material_height, w inutilisé
    texture_warp: Vec4,   // x = warp_amplitude, y = warp_wavelength, zw inutilisés
    texture_tint: Vec4,   // x = tint_wavelength, y = brightness_variation, z = hue_variation, w inutilisé
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

    pub fn update(
        &self,
        camera: &Camera,
        sun: &DirectionalLight,
        atmosphere: &Atmosphere,
        stars: &Stars,
        world_maps_uv: Vec4,
        water: Option<&Water>,
        config: &EngineConfig,
        real_time_secs: f32,
    ) {
        profile!();
        let coast_moisture = &config.material_rules.coast_moisture;
        let tonemap = &config.render.tonemap;
        let textures = &config.material_textures;
        let shadow = &config.render.shadow;
        let atmo = &atmosphere.params;
        // Sans mer, w = 0 : les shaders ne testent rien d'autre.
        let (water_params, water_absorption, water_deep_color) = match water {
            Some(w) => (
                Vec4::new(w.sea_level, w.max_distance, LAND_MARGIN, 1.0),
                w.params
                    .absorption
                    .extend(if w.camera_under_sea { 1.0 } else { 0.0 }),
                w.params.deep_color.extend(w.params.glint_slope_variance),
            ),
            None => (Vec4::ZERO, Vec4::ZERO, Vec4::ZERO),
        };
        // Sans mer : bornes 0 et 1, jamais lues (w = 0 dans `water`), mais pas de
        // division par zéro dans le smoothstep.
        let (water_deep_cold, water_sst) = water.map_or((Vec4::ZERO, Vec4::Y), |w| {
            (
                w.params.deep_color_cold.extend(0.0),
                Vec4::new(w.params.sst_cold, w.params.sst_warm, 0.0, 0.0),
            )
        });
        // Sans mer, amplitudes nulles.
        let (waves, wave_params) = water.map_or(
            ([Vec4::ZERO; 2 * WAVE_COUNT], Vec4::ZERO),
            |w| w.waves.gpu_data(),
        );
        let foam = water.map_or([Vec4::ZERO; 3], |w| {
            let f = &w.params.foam;
            [
                Vec4::new(f.albedo, f.zone_depth, f.patch_size, f.bubble_size),
                Vec4::new(f.bubble_speed, f.drift, f.surge_period, f.surge_spread),
                Vec4::new(f.calm_zone, f.calm_density, 0.0, 0.0),
            ]
        });
        let ubo = UniformBufferObject {
            view: camera.view,
            proj: camera.proj,
            camera_position: camera.position.extend(1.0),
            light_view_proj: sun.view_proj(camera.position, camera.front()),
            light_direction: sun.direction.extend(0.0),
            light_color: sun.color.extend(sun.intensity),
            fog: atmosphere.horizon_color.extend(atmo.fog_density),
            shadow_params: Vec4::new(
                sun.shadow.depth_range(),
                sun.shadow_texel_size(),
                // Résolution réelle de la shadow map (celle de sa création).
                1.0 / sun.shadow_map_resolution as f32,
                if config.debug.shadows { 1.0 } else { 0.0 },
            ),
            shadow_bias: Vec4::new(
                shadow.normal_offset_texels,
                shadow.depth_bias_texels,
                0.0,
                0.0,
            ),
            fog_params: Vec4::new(
                atmo.fog_scale_height,
                atmo.fog_anisotropy,
                atmo.halo_strength,
                0.0,
            ),
            sun_disk: Vec4::new(
                sun.disk.angular_radius_deg.to_radians(),
                sun.disk.edge_softness,
                sun.disk.intensity,
                0.0,
            ),
            sky_zenith: atmosphere.zenith_color.extend(atmo.sky_gradient_exponent),
            ambient: atmosphere.ambient_color.extend(atmo.ground_bounce),
            sky_rotation: Mat4::from_mat3(stars.world_to_sky),
            stars: Vec4::new(
                stars.params.brightness * stars.visibility,
                stars.params.radius_px,
                stars.params.density,
                stars.params.twinkle,
            ),
            world_maps: world_maps_uv,
            water: water_params,
            water_absorption,
            water_deep_color,
            water_deep_cold,
            water_sst,
            time: Vec4::new(real_time_secs, 0.0, 0.0, 0.0),
            waves,
            wave_params,
            foam,
            material_tint: Vec4::new(coast_moisture.amount, coast_moisture.range, 0.0, 0.0),
            tonemap: Vec4::new(
                if tonemap.enabled { 1.0 } else { 0.0 },
                tonemap.exposure,
                0.0,
                0.0,
            ),
            texture_blend: Vec4::new(
                textures.triplanar_sharpness,
                textures.projection_blend_depth,
                textures.material_blend_depth,
                0.0,
            ),
            texture_detail: Vec4::new(
                textures.normal_strength,
                textures.ao_strength,
                textures.flat_material_height,
                0.0,
            ),
            texture_warp: Vec4::new(textures.warp_amplitude, textures.warp_wavelength, 0.0, 0.0),
            texture_tint: Vec4::new(
                textures.tint_wavelength,
                textures.brightness_variation,
                textures.hue_variation,
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
