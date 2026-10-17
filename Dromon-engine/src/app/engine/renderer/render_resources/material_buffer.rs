use anyhow::Result;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use glam::Vec4;
use std::sync::Arc;

use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::renderer::descriptors::DescriptorHandler;
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
}

impl GpuMaterial {
    fn new(def: &MaterialDef) -> GpuMaterial {
        let (dry_albedo, dry, wet) = match def.humidity_tint {
            Some(t) => (t.dry_albedo, t.dry, t.wet),
            None => (def.albedo, 0.0, 1.0),
        };
        GpuMaterial {
            albedo: def.albedo.extend(0.0),
            dry_albedo: dry_albedo.extend(0.0),
            humidity: Vec4::new(dry, wet, 0.0, 0.0),
        }
    }

    /// Couleur fixe, sans teinte.
    fn plain(albedo: Vec4) -> GpuMaterial {
        GpuMaterial {
            albedo,
            dry_albedo: albedo,
            humidity: Vec4::new(0.0, 1.0, 0.0, 0.0),
        }
    }
}

/// **Table des matériaux** sur le GPU (SSBO, set 4), indexée par ID : case 0 = air,
/// puis `materials.ron` dans l'ordre — même convention que `MaterialTable` —, puis le
/// matériau de debug (magenta, cf. `MaterialsConfig::debug_id`).
///
/// Statique : construite avec le terrain, envoyée dans la passe de transfert du
/// démarrage.
pub struct MaterialBuffer {
    staging: Buffer,
    buffer: Buffer,
    size: vk::DeviceSize,
    pub descriptor_set: vk::DescriptorSet,
}

impl MaterialBuffer {
    pub fn new(
        context: Arc<RenderingContext>,
        descriptor_handler: &DescriptorHandler,
        config: &MaterialsConfig,
    ) -> Result<MaterialBuffer> {
        let air = GpuMaterial::plain(Vec4::ZERO);
        let debug = GpuMaterial::plain(Vec4::new(1.0, 0.0, 1.0, 0.0));
        let materials: Vec<GpuMaterial> = std::iter::once(air)
            .chain(config.materials.iter().map(GpuMaterial::new))
            .chain(std::iter::once(debug))
            .collect();
        debug_assert_eq!(materials.len() - 1, config.debug_id() as usize);
        let size = std::mem::size_of_val(materials.as_slice()) as vk::DeviceSize;

        let staging = Buffer::new(
            context.clone(),
            size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        staging.upload(&materials)?;
        // Device-local : lu par chaque fragment, il ne doit pas traverser le PCIe.
        let buffer = Buffer::new(
            context,
            size,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;
        let descriptor_set = descriptor_handler.create_materials_descriptor_set(buffer.buffer)?;

        Ok(MaterialBuffer {
            staging,
            buffer,
            size,
            descriptor_set,
        })
    }

    /// Copie staging → device. Passe de transfert du démarrage uniquement (suivie
    /// d'une attente complète de la queue : pas de barrière ici).
    pub fn record_upload(&self, device: &ash::Device, command_buffer: vk::CommandBuffer) {
        unsafe {
            device.cmd_copy_buffer(
                command_buffer,
                self.staging.buffer,
                self.buffer.buffer,
                &[vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: self.size,
                }],
            );
        }
    }
}
