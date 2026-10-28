use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::{Vec3, Vec4};
use std::sync::Arc;

use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::renderer::descriptors::DescriptorHandler;
use crate::app::engine::renderer::render_resources::terrain_textures::TerrainTextures;
use crate::app::engine::renderer::render_resources::texture_handler::TextureHandler;
use crate::app::engine::renderer::renderer_initialize::submit_one_time;
use crate::app::engine::rendering_context::RenderingContext;
use crate::config::{MaterialDef, MaterialsConfig};

/// Un matériau tel que le lit le GPU. ⚠ Même disposition que `Material` dans
/// `lib/materials.slang` (std430 : membres alignés sur 16 octets, d'où des `Vec4`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuMaterial {
    /// rgb = albédo linéaire (humide si teinté), w inutilisé (rugosité à venir).
    albedo: Vec4,
    /// rgb = albédo en zone sèche ; égal à `albedo` si le matériau n'est pas teinté
    /// (le shader fait le même fondu pour tous, sans branchement). w inutilisé.
    dry_albedo: Vec4,
    /// x, y = précipitations du fondu sec → humide ; zw inutilisés.
    humidity: Vec4,
    /// x = couche du texture array (−1 : pas de texture), y = 1 / `texture_size` ; zw
    /// inutilisés.
    texture_params: Vec4,
    /// rgb = 1 / albédo moyen de la texture : `texel / moyenne` ne garde que le
    /// **détail** de la texture, la couleur d'ensemble vient de la table. w inutilisé.
    texture_inv_mean: Vec4,
}

/// Pas de texture : couche −1 (le shader garde la couleur unie).
const NO_TEXTURE: Vec4 = Vec4::new(-1.0, 0.0, 0.0, 0.0);

/// Plancher de la moyenne avant inversion (texture noire sur un canal).
const MIN_TEXTURE_MEAN: f32 = 1e-3;

impl GpuMaterial {
    /// `texture` : couche du texture array et albédo moyen de cette couche.
    fn new(def: &MaterialDef, texture: Option<(u32, Vec3)>) -> GpuMaterial {
        let (dry_albedo, dry, wet) = match def.humidity_tint {
            Some(t) => (t.dry_albedo, t.dry, t.wet),
            None => (def.albedo, 0.0, 1.0),
        };
        let (texture_params, texture_inv_mean) = match texture {
            Some((layer, mean)) => (
                Vec4::new(layer as f32, 1.0 / def.texture_size, 0.0, 0.0),
                mean.max(Vec3::splat(MIN_TEXTURE_MEAN)).recip().extend(0.0),
            ),
            None => (NO_TEXTURE, Vec4::ONE),
        };
        GpuMaterial {
            albedo: def.albedo.extend(0.0),
            dry_albedo: dry_albedo.extend(0.0),
            humidity: Vec4::new(dry, wet, 0.0, 0.0),
            texture_params,
            texture_inv_mean,
        }
    }

    /// Couleur fixe, sans teinte.
    fn plain(albedo: Vec4) -> GpuMaterial {
        GpuMaterial {
            albedo,
            dry_albedo: albedo,
            humidity: Vec4::new(0.0, 1.0, 0.0, 0.0),
            texture_params: NO_TEXTURE,
            texture_inv_mean: Vec4::ONE,
        }
    }
}

/// **Table des matériaux** sur le GPU (SSBO, set 4), indexée par ID : case 0 = air,
/// puis `materials.ron` dans l'ordre — même convention que `MaterialTable` —, puis le
/// matériau de debug (magenta, cf. `MaterialsConfig::debug_id`).
///
/// Porte aussi les textures du terrain (bindings 1 = albédo, 2 = normales du même set).
///
/// Construite avec le terrain, envoyée dans la passe de transfert du démarrage ;
/// reconstruite par le hot reload de `materials.ron` ou des fichiers de texture.
pub struct MaterialBuffer {
    staging: Buffer,
    buffer: Buffer,
    size: vk::DeviceSize,
    textures: TerrainTextures,
    pub descriptor_set: vk::DescriptorSet,
}

impl MaterialBuffer {
    pub fn new(
        context: Arc<RenderingContext>,
        texture_handler: &TextureHandler,
        descriptor_handler: &DescriptorHandler,
        config: &MaterialsConfig,
    ) -> Result<MaterialBuffer> {
        // D'abord les textures : la table a besoin de leurs albédos moyens.
        let textures = TerrainTextures::new(texture_handler, config)?;
        let (staging, buffer, size) = create_buffers(context, &gpu_table(config, &textures))?;
        let descriptor_set = descriptor_handler.create_materials_descriptor_set(
            buffer.buffer,
            textures.albedo_view(),
            textures.normal_view(),
            texture_handler.base_texture_sampler,
        )?;

        Ok(MaterialBuffer {
            staging,
            buffer,
            size,
            textures,
            descriptor_set,
        })
    }

    /// Copie staging → device (table et textures). Passe de transfert du démarrage
    /// uniquement (suivie d'une attente complète de la queue : pas de barrière ici).
    pub fn record_upload(
        &self,
        texture_handler: &TextureHandler,
        command_buffer: &vk::CommandBuffer,
    ) -> Result<()> {
        record_copy(texture_handler, *command_buffer, &self.staging, &self.buffer, self.size);
        self.textures.record_upload(texture_handler, command_buffer)
    }

    /// Hot reload : nouvelle table depuis `config`, et nouvelles textures si
    /// `reload_textures` (liste changée ou fichiers modifiés). Envoyées aussitôt, puis le
    /// même set est réécrit (le pool ne libère pas ses sets). Aucune frame en vol ne doit
    /// lire la table : à l'appelant d'attendre le GPU. En cas d'erreur, l'ancien contenu
    /// reste en place, intact.
    pub fn hot_reload(
        &mut self,
        texture_handler: &TextureHandler,
        descriptor_handler: &DescriptorHandler,
        config: &MaterialsConfig,
        reload_textures: bool,
    ) -> Result<()> {
        let mut textures = if reload_textures {
            Some(TerrainTextures::new(texture_handler, config)?)
        } else {
            None
        };
        let table = gpu_table(config, textures.as_ref().unwrap_or(&self.textures));
        let context = &texture_handler.context;
        let (staging, buffer, size) = create_buffers(context.clone(), &table)?;
        submit_one_time(context, |command_buffer| {
            record_copy(texture_handler, command_buffer, &staging, &buffer, size);
            match textures.as_ref() {
                Some(textures) => textures.record_upload(texture_handler, &command_buffer),
                None => Ok(()),
            }
        })?;

        // Envoyé : on remplace (les anciens buffers et textures sont détruits ici).
        self.staging = staging;
        self.buffer = buffer;
        self.size = size;
        if let Some(mut textures) = textures.take() {
            textures.release_texture_staging();
            self.textures = textures;
        }
        descriptor_handler.write_materials_buffer(self.descriptor_set, self.buffer.buffer);
        self.hot_reload_sampler(descriptor_handler, texture_handler.base_texture_sampler);
        Ok(())
    }

    /// Hot reload du sampler des textures.
    pub fn hot_reload_sampler(&self, descriptor_handler: &DescriptorHandler, sampler: vk::Sampler) {
        descriptor_handler.write_materials_textures(
            self.descriptor_set,
            self.textures.albedo_view(),
            self.textures.normal_view(),
            sampler,
        );
    }

    /// Libère le staging des textures, une fois la passe de transfert exécutée.
    pub fn release_texture_staging(&mut self) {
        self.textures.release_texture_staging();
    }
}

/// Table GPU, indexée par ID : air, `config` dans l'ordre, puis debug.
fn gpu_table(config: &MaterialsConfig, textures: &TerrainTextures) -> Vec<GpuMaterial> {
    let air = GpuMaterial::plain(Vec4::ZERO);
    let debug = GpuMaterial::plain(Vec4::new(1.0, 0.0, 1.0, 0.0));
    let materials: Vec<GpuMaterial> = std::iter::once(air)
        .chain(config.materials.iter().map(|m| {
            let texture = config.texture_layer(m).map(|l| (l, textures.mean(l)));
            GpuMaterial::new(m, texture)
        }))
        .chain(std::iter::once(debug))
        .collect();
    debug_assert_eq!(materials.len() - 1, config.debug_id() as usize);
    materials
}

/// Staging rempli de `table`, et buffer device de même taille.
fn create_buffers(
    context: Arc<RenderingContext>,
    table: &[GpuMaterial],
) -> Result<(Buffer, Buffer, vk::DeviceSize)> {
    let size = std::mem::size_of_val(table) as vk::DeviceSize;
    let staging = Buffer::new(
        context.clone(),
        size,
        vk::BufferUsageFlags::TRANSFER_SRC,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;
    staging.upload(table)?;
    // Device-local : lu par chaque fragment, il ne doit pas traverser le PCIe.
    let buffer = Buffer::new(
        context,
        size,
        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::STORAGE_BUFFER,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )?;
    Ok((staging, buffer, size))
}

fn record_copy(
    texture_handler: &TextureHandler,
    command_buffer: vk::CommandBuffer,
    staging: &Buffer,
    buffer: &Buffer,
    size: vk::DeviceSize,
) {
    unsafe {
        texture_handler.context.device.cmd_copy_buffer(
            command_buffer,
            staging.buffer,
            buffer.buffer,
            &[vk::BufferCopy {
                src_offset: 0,
                dst_offset: 0,
                size,
            }],
        );
    }
}
