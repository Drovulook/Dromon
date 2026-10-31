//! **Captures d'écran** : relecture (*readback*) de l'image de la swapchain après le rendu.
//! - [`Screenshots::record`] (GPU) : image → buffer host-visible, dans la frame courante.
//! - [`Screenshots::collect`] (CPU) : au retour sur le même slot, sa fence est passée →
//!   lecture du buffer, puis conversion et PNG sur un thread. Aucune attente ajoutée : la
//!   capture arrive `frames in flight` frames plus tard.

use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::renderer::image_layout_state::ImageLayoutState;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::{Context, Result, bail};
use ash::vk;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Appelé une fois la capture écrite (chemin) ou abandonnée (erreur), depuis un autre
/// thread.
pub(crate) type ScreenshotDone = Box<dyn FnOnce(Result<PathBuf>) + Send>;

pub(crate) struct ScreenshotRequest {
    /// PNG à écrire (dossiers créés au besoin).
    pub path: PathBuf,
    pub done: ScreenshotDone,
}

/// Copie enregistrée dans une frame, pas encore relue.
struct InFlight {
    request: ScreenshotRequest,
    buffer: Buffer,
    extent: vk::Extent2D,
    format: vk::Format,
}

pub(crate) struct Screenshots {
    context: Arc<RenderingContext>,
    queued: VecDeque<ScreenshotRequest>,
    /// Une case par frame in flight : la copie que ce slot attend de relire.
    in_flight: Vec<Option<InFlight>>,
}

impl Screenshots {
    pub fn new(context: Arc<RenderingContext>, frames_in_flight: usize) -> Self {
        Self {
            context,
            queued: VecDeque::new(),
            in_flight: (0..frames_in_flight).map(|_| None).collect(),
        }
    }

    pub fn request(&mut self, request: ScreenshotRequest) {
        self.queued.push_back(request);
    }

    /// Fait passer `image` (cible de rendu) en état présentable, en la copiant au passage
    /// si une capture attend. Une capture par frame au plus.
    pub fn record(
        &mut self,
        command_buffer: vk::CommandBuffer,
        frame_index: usize,
        image: vk::Image,
        extent: vk::Extent2D,
        format: vk::Format,
    ) {
        // Slot occupé : sa copie précédente n'a pas encore été relue.
        let request = if self.in_flight[frame_index].is_none() {
            self.queued.pop_front()
        } else {
            None
        };
        let Some(request) = request else {
            return self.present(command_buffer, image);
        };
        let buffer = match readback_buffer(&self.context, extent) {
            Ok(buffer) => buffer,
            Err(e) => {
                (request.done)(Err(e));
                return self.present(command_buffer, image);
            }
        };

        self.context.transition_image_layout(
            command_buffer,
            &[(
                image,
                ImageLayoutState::RENDERABLE_COLOR_IMAGE_STATE,
                ImageLayoutState::COPY_SOURCE_COLOR_IMAGE_STATE,
            )],
            1,
        );
        unsafe {
            // `buffer_row_length` 0 : lignes contiguës, `width × 4` octets.
            self.context.device.cmd_copy_image_to_buffer(
                command_buffer,
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
                        width: extent.width,
                        height: extent.height,
                        depth: 1,
                    })],
            );
            // La fence garantit que la copie est finie, pas que le CPU voit ses
            // écritures : il faut aussi cette barrière vers HOST_READ.
            self.context.device.cmd_pipeline_barrier2(
                command_buffer,
                &vk::DependencyInfo::default().buffer_memory_barriers(&[
                    vk::BufferMemoryBarrier2::default()
                        .src_stage_mask(vk::PipelineStageFlags2::COPY)
                        .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                        .dst_stage_mask(vk::PipelineStageFlags2::HOST)
                        .dst_access_mask(vk::AccessFlags2::HOST_READ)
                        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .buffer(buffer.buffer)
                        .size(vk::WHOLE_SIZE),
                ]),
            );
        }
        self.context.transition_image_layout(
            command_buffer,
            &[(
                image,
                ImageLayoutState::COPY_SOURCE_COLOR_IMAGE_STATE,
                ImageLayoutState::PRESENT_COLOR_IMAGE_STATE,
            )],
            1,
        );

        self.in_flight[frame_index] = Some(InFlight {
            request,
            buffer,
            extent,
            format,
        });
    }

    /// Transition habituelle, sans capture.
    fn present(&self, command_buffer: vk::CommandBuffer, image: vk::Image) {
        self.context.transition_image_layout(
            command_buffer,
            &[(
                image,
                ImageLayoutState::RENDERABLE_COLOR_IMAGE_STATE,
                ImageLayoutState::PRESENT_COLOR_IMAGE_STATE,
            )],
            1,
        );
    }

    /// À appeler après la fence du slot `frame_index` : relit sa copie s'il en a une,
    /// puis confie conversion et écriture du PNG à un thread (~0,1 s en 1080p).
    pub fn collect(&mut self, frame_index: usize) {
        let Some(InFlight {
            request,
            buffer,
            extent,
            format,
        }) = self.in_flight[frame_index].take()
        else {
            return;
        };
        let pixels = match read_pixels(&buffer, extent) {
            Ok(pixels) => pixels,
            Err(e) => return (request.done)(Err(e)),
        };
        // Le buffer est libéré ici ; le thread ne garde que la copie CPU.
        drop(buffer);
        std::thread::spawn(move || {
            let result = save_png(&request.path, pixels, extent, format);
            (request.done)(result.map(|()| request.path));
        });
    }
}

/// Mémoire relue par le CPU : `HOST_CACHED` de préférence (lire de la mémoire non
/// cachée, faite pour les envois CPU → GPU, est très lent), sinon la mémoire de staging.
fn readback_buffer(context: &Arc<RenderingContext>, extent: vk::Extent2D) -> Result<Buffer> {
    let size = extent.width as vk::DeviceSize * extent.height as vk::DeviceSize * 4;
    let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    let usage = vk::BufferUsageFlags::TRANSFER_DST;
    Buffer::new(context.clone(), size, usage, host | vk::MemoryPropertyFlags::HOST_CACHED)
        .or_else(|_| Buffer::new(context.clone(), size, usage, host))
        .context("buffer de capture")
}

/// Copie CPU du contenu du buffer (mémoire cohérente : rien à invalider).
fn read_pixels(buffer: &Buffer, extent: vk::Extent2D) -> Result<Vec<u8>> {
    let len = extent.width as usize * extent.height as usize * 4;
    let ptr = buffer.map()?;
    // SAFETY : le buffer fait `len` octets (cf. `readback_buffer`), est mappé tant qu'il
    // vit, et le GPU a fini d'y écrire (fence du slot passée).
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec())
}

/// Texels 4 octets → PNG RGB. Les valeurs sont déjà encodées en sRGB (format `_SRGB`),
/// ce qu'attend un PNG : seul l'ordre des canaux change. L'alpha est ignoré.
fn save_png(path: &Path, pixels: Vec<u8>, extent: vk::Extent2D, format: vk::Format) -> Result<()> {
    let bgra = match format {
        vk::Format::B8G8R8A8_SRGB | vk::Format::B8G8R8A8_UNORM => true,
        vk::Format::R8G8B8A8_SRGB | vk::Format::R8G8B8A8_UNORM => false,
        other => bail!("format de swapchain non géré pour la capture : {other:?}"),
    };
    let rgb: Vec<u8> = pixels
        .chunks_exact(4)
        .flat_map(|p| if bgra { [p[2], p[1], p[0]] } else { [p[0], p[1], p[2]] })
        .collect();
    let image = image::RgbImage::from_raw(extent.width, extent.height, rgb)
        .context("taille d'image incohérente")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
    }
    image
        .save(path)
        .with_context(|| format!("écriture de {}", path.display()))
}
