use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

pub struct Buffer {
    context: Arc<RenderingContext>,
    pub buffer: vk::Buffer,
    buffer_memory: vk::DeviceMemory,
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

        let buffer_memory = unsafe {
            context.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(memory_requirements.size)
                    .memory_type_index(context.find_memory_type(
                        memory_requirements.memory_type_bits,
                        memory_properties,
                    )?),
                None,
            )?
        };

        unsafe {
            context
                .device
                .bind_buffer_memory(buffer, buffer_memory, 0)?;
        }

        Ok(Self {
            context,
            buffer,
            buffer_memory,
        })
    }

    pub fn map_and_unmap<T: Copy>(&self, buffer_data: &[T]) -> Result<()> {
        unsafe {
            let data = self.context.device.map_memory(
                self.buffer_memory,
                0,
                std::mem::size_of_val(buffer_data) as u64,
                vk::MemoryMapFlags::empty(),
            )? as *mut T;
            std::ptr::copy_nonoverlapping(buffer_data.as_ptr(), data, buffer_data.len());
            self.context.device.unmap_memory(self.buffer_memory);
        }
        Ok(())
    }

    pub fn map(&self, size: vk::DeviceSize) -> Result<*mut std::ffi::c_void> {
        let ptr = unsafe {
            self.context.device.map_memory(
                self.buffer_memory,
                0,
                size,
                vk::MemoryMapFlags::empty(),
            )?
        };
        Ok(ptr)
    }

    pub fn unmap(&self) {
        unsafe { self.context.device.unmap_memory(self.buffer_memory) };
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.context.device.device_wait_idle();
            self.context.device.destroy_buffer(self.buffer, None);
            self.context.device.free_memory(self.buffer_memory, None);
        }
    }
}
