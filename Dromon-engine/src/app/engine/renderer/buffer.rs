use crate::app::engine::{
    renderer::device_memory::Allocation, rendering_context::RenderingContext,
};
use anyhow::{Context as _, Result};
use ash::vk;
use std::sync::Arc;

pub struct Buffer {
    context: Arc<RenderingContext>,
    pub buffer: vk::Buffer,
    allocation: Allocation,
}

impl Buffer {
    pub fn new(
        context: Arc<RenderingContext>,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
        memory_properties: vk::MemoryPropertyFlags,
    ) -> Result<Self> {
        let buffer = unsafe {
            context.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?
        };

        let memory_requirements = unsafe { context.device.get_buffer_memory_requirements(buffer) };

        let allocation =
            context
                .allocator()
                .allocate(&context, memory_requirements, memory_properties)?;

        unsafe {
            context
                .device
                .bind_buffer_memory(buffer, allocation.memory, allocation.offset)?;
        }

        Ok(Self {
            context,
            buffer,
            allocation,
        })
    }

    /// Écrit `buffer_data` dans le buffer. Le bloc étant mappé en permanence, il ne
    /// reste qu'un `memcpy` — plus aucun appel au driver.
    pub fn upload<T: Copy>(&self, buffer_data: &[T]) -> Result<()> {
        let bytes = std::mem::size_of_val(buffer_data);
        // Déborder n'écrase plus une zone à soi mais la tranche du voisin : géométrie
        // corrompue, sans erreur de validation pour le signaler.
        debug_assert!(bytes as u64 <= self.allocation.size);

        let destination = self.map()?;
        unsafe {
            // Copie octet par octet : `copy_nonoverlapping::<T>` exigerait un `destination`
            // aligné pour `T`, ce que `MemoryRequirements::alignment` ne promet pas.
            std::ptr::copy_nonoverlapping(buffer_data.as_ptr() as *const u8, destination, bytes);
        }
        Ok(())
    }

    /// Pointeur CPU vers le contenu du buffer, valide tant que le [`Buffer`] vit.
    /// Rien à démapper : le mapping appartient au bloc, pas à la tranche.
    pub fn map(&self) -> Result<*mut u8> {
        self.context
            .allocator()
            .mapped_ptr(&self.allocation)
            .context("pointeur CPU demandé sur un buffer qui n'est pas host-visible")
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            self.context.device.destroy_buffer(self.buffer, None);
            self.context.allocator().free_alloc(&self.allocation);
        }
    }
}
