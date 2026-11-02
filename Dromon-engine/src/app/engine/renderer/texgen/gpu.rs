//! Exécution d'une recette sur le GPU : 2 passes compute dans 3 storage images, puis
//! relecture CPU. Tout est créé et détruit à chaque appel (outil, pas boucle de rendu).

use super::compile::RecipeShaders;
use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::renderer::image_layout_state::ImageLayoutState;
use crate::app::engine::renderer::pipelines::{create_compute_pipeline, create_shader_module};
use crate::app::engine::renderer::render_resources::texture::Texture;
use crate::app::engine::renderer::renderer_initialize::submit_one_time;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::{Result, ensure};
use ash::vk;
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;

/// ⚠ = `numthreads(8, 8, 1)` du harnais (procgen/texgen.slang).
const WORKGROUP_SIZE: u32 = 8;
/// ⚠ Formats synchronisés avec les `[format(…)]` du harnais.
const HEIGHT_FORMAT: vk::Format = vk::Format::R32_SFLOAT;
const RGBA_FORMAT: vk::Format = vk::Format::R32G32B32A32_SFLOAT;

/// ⚠ Disposition = `TexgenPushConstants` du harnais.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PushConstants {
    size: u32,
    seed: u32,
}

/// Cartes générées, texels rangés ligne par ligne (`size × size`).
pub(super) struct GeneratedMaps {
    pub size: u32,
    /// Unités de tuile, non normalisée.
    pub height: Vec<f32>,
    /// rgb linéaire, a inutilisé.
    pub albedo: Vec<[f32; 4]>,
    /// xyz = normale (NormalGL, unitaire), w = AO.
    pub normal_ao: Vec<[f32; 4]>,
}

/// Exécute les deux passes de `shaders` sur une tuile `size × size` et relit le résultat.
/// Bloquant : attend la fin du GPU.
pub(super) fn generate(
    context: &Arc<RenderingContext>,
    shaders: &RecipeShaders,
    size: u32,
    seed: u32,
) -> Result<GeneratedMaps> {
    ensure!(size > 0, "taille de texture nulle");
    let pipelines = Pipelines::new(context, shaders)?;
    let height = storage_image(context, size, HEIGHT_FORMAT)?;
    let albedo = storage_image(context, size, RGBA_FORMAT)?;
    let normal_ao = storage_image(context, size, RGBA_FORMAT)?;
    pipelines.write_images([&height, &albedo, &normal_ao]);

    // Un buffer par image : chacun tient dans un bloc standard de l'allocateur (64 Mo,
    // soit une image RGBA32F 2048²).
    let texels = size as usize * size as usize;
    let height_buffer = Buffer::readback(context, (texels * 4) as vk::DeviceSize)?;
    let albedo_buffer = Buffer::readback(context, (texels * 16) as vk::DeviceSize)?;
    let normal_ao_buffer = Buffer::readback(context, (texels * 16) as vk::DeviceSize)?;

    let push = PushConstants { size, seed };
    let groups = size.div_ceil(WORKGROUP_SIZE);
    let (h, a, n) = (height.texture_image, albedo.texture_image, normal_ao.texture_image);
    submit_one_time(context, |cb| {
        let device = &context.device;
        use ImageLayoutState as S;
        context.transition_image_layout(
            cb,
            &[
                (h, S::UNDEFINED_COLOR_IMAGE_STATE, S::STORAGE_WRITE_STATE),
                (a, S::UNDEFINED_COLOR_IMAGE_STATE, S::STORAGE_WRITE_STATE),
                (n, S::UNDEFINED_COLOR_IMAGE_STATE, S::STORAGE_WRITE_STATE),
            ],
            1,
        );
        unsafe {
            // Set et push constants restent liés d'une pipeline à l'autre : même layout.
            device.cmd_bind_descriptor_sets(
                cb,
                vk::PipelineBindPoint::COMPUTE,
                pipelines.layout,
                0,
                &[pipelines.set],
                &[],
            );
            device.cmd_push_constants(
                cb,
                pipelines.layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                bytemuck::bytes_of(&push),
            );
            // Passe 1 : hauteur + albédo.
            device.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, pipelines.surface);
            device.cmd_dispatch(cb, groups, groups, 1);
        }
        // La passe 2 lit la hauteur écrite par la passe 1.
        context.transition_image_layout(
            cb,
            &[(h, S::STORAGE_WRITE_STATE, S::STORAGE_READ_STATE)],
            1,
        );
        unsafe {
            // Passe 2 : normale + AO.
            device.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, pipelines.normal_ao);
            device.cmd_dispatch(cb, groups, groups, 1);
        }
        context.transition_image_layout(
            cb,
            &[
                (h, S::STORAGE_READ_STATE, S::COPY_SOURCE_COLOR_IMAGE_STATE),
                (a, S::STORAGE_WRITE_STATE, S::COPY_SOURCE_COLOR_IMAGE_STATE),
                (n, S::STORAGE_WRITE_STATE, S::COPY_SOURCE_COLOR_IMAGE_STATE),
            ],
            1,
        );
        let copies = [(h, &height_buffer), (a, &albedo_buffer), (n, &normal_ao_buffer)];
        for (image, buffer) in copies {
            unsafe {
                device.cmd_copy_image_to_buffer(
                    cb,
                    image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buffer.buffer,
                    &[vk::BufferImageCopy::default()
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )
                        .image_extent(vk::Extent3D {
                            width: size,
                            height: size,
                            depth: 1,
                        })],
                );
            }
        }
        // La fin d'exécution ne suffit pas : cette barrière rend les copies visibles au
        // CPU (comme pour les captures d'écran).
        let host_barriers = copies.map(|(_, buffer)| {
            vk::BufferMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::COPY)
                .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::HOST)
                .dst_access_mask(vk::AccessFlags2::HOST_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(buffer.buffer)
                .size(vk::WHOLE_SIZE)
        });
        unsafe {
            device.cmd_pipeline_barrier2(
                cb,
                &vk::DependencyInfo::default().buffer_memory_barriers(&host_barriers),
            );
        }
        Ok(())
    })?;

    Ok(GeneratedMaps {
        size,
        height: read_texels(&height_buffer, texels)?,
        albedo: read_texels(&albedo_buffer, texels)?,
        normal_ao: read_texels(&normal_ao_buffer, texels)?,
    })
}

/// Image `size × size` écrite par les compute shaders puis copiée vers un buffer.
fn storage_image(
    context: &Arc<RenderingContext>,
    size: u32,
    format: vk::Format,
) -> Result<Texture> {
    let (image, memory) = context.create_image(
        size,
        size,
        1,
        vk::SampleCountFlags::TYPE_1,
        format,
        vk::ImageTiling::OPTIMAL,
        vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )?;
    let view = context.create_image_view(&image, format, vk::ImageAspectFlags::COLOR, 1)?;
    Ok(Texture::new(
        context.clone(),
        size,
        size,
        1,
        1,
        format,
        image,
        memory,
        view,
        None,
    ))
}

/// Copie les `count` premiers texels de `buffer` (relu après la fin du GPU).
fn read_texels<T: Pod>(buffer: &Buffer, count: usize) -> Result<Vec<T>> {
    let len = count * std::mem::size_of::<T>();
    let ptr = buffer.map()?;
    // SAFETY : le buffer fait au moins `len` octets (cf. `generate`), est mappé tant
    // qu'il vit, et le GPU a fini d'y écrire (`submit_one_time` attend la queue).
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    // Copie octet à octet : le mapping ne garantit pas l'alignement de `T`.
    Ok(bytemuck::pod_collect_to_vec(bytes))
}

/// Objets Vulkan des deux passes, détruits ensemble (`Drop`) — y compris si la création
/// échoue en route : les champs encore nuls sont ignorés par Vulkan.
struct Pipelines {
    context: Arc<RenderingContext>,
    /// 3 storage images (hauteur, albédo, normale+AO), bindings 0 à 2.
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    surface: vk::Pipeline,
    normal_ao: vk::Pipeline,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
}

impl Pipelines {
    fn new(context: &Arc<RenderingContext>, shaders: &RecipeShaders) -> Result<Self> {
        let mut p = Self {
            context: context.clone(),
            set_layout: vk::DescriptorSetLayout::null(),
            layout: vk::PipelineLayout::null(),
            surface: vk::Pipeline::null(),
            normal_ao: vk::Pipeline::null(),
            pool: vk::DescriptorPool::null(),
            set: vk::DescriptorSet::null(),
        };
        let device = &context.device;

        let bindings: [_; 3] = std::array::from_fn(|i| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(i as u32)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
        });
        p.set_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?
        };
        let push_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(std::mem::size_of::<PushConstants>() as u32)];
        p.layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[p.set_layout])
                    .push_constant_ranges(&push_ranges),
                None,
            )?
        };
        p.surface = compute_pipeline(device, &shaders.surface, p.layout)?;
        p.normal_ao = compute_pipeline(device, &shaders.normal_ao, p.layout)?;

        p.pool = unsafe {
            device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .pool_sizes(&[vk::DescriptorPoolSize::default()
                        .ty(vk::DescriptorType::STORAGE_IMAGE)
                        .descriptor_count(3)])
                    .max_sets(1),
                None,
            )?
        };
        p.set = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(p.pool)
                    .set_layouts(&[p.set_layout]),
            )?[0]
        };
        Ok(p)
    }

    /// Image `i` → binding `i`, en layout GENERAL (celui des storage images).
    fn write_images(&self, images: [&Texture; 3]) {
        let infos = images.map(|image| {
            [vk::DescriptorImageInfo::default()
                .image_view(image.texture_image_view)
                .image_layout(vk::ImageLayout::GENERAL)]
        });
        let writes: Vec<_> = infos
            .iter()
            .enumerate()
            .map(|(binding, info)| {
                vk::WriteDescriptorSet::default()
                    .dst_set(self.set)
                    .dst_binding(binding as u32)
                    .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                    .image_info(info)
            })
            .collect();
        unsafe { self.context.device.update_descriptor_sets(&writes, &[]) };
    }
}

impl Drop for Pipelines {
    fn drop(&mut self) {
        let device = &self.context.device;
        unsafe {
            // Détruire le pool libère son set.
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_pipeline(self.surface, None);
            device.destroy_pipeline(self.normal_ao, None);
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
        }
    }
}

/// Pipeline compute d'un SPIR-V ; le module ne sert qu'à la création.
fn compute_pipeline(
    device: &ash::Device,
    code: &[u32],
    layout: vk::PipelineLayout,
) -> Result<vk::Pipeline> {
    let module = create_shader_module(device, code)?;
    let pipeline = create_compute_pipeline(device, module, layout, vk::PipelineCache::null());
    unsafe { device.destroy_shader_module(module, None) };
    pipeline
}
