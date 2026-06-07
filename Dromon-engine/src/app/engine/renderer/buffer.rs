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
                    .memory_type_index(Buffer::find_memory_type(
                        &context,
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

    fn find_memory_type(
        context: &RenderingContext,
        type_filter: u32,
        properties: vk::MemoryPropertyFlags,
    ) -> Result<u32> {
        let memory_properties = unsafe {
            context
                .instance
                .get_physical_device_memory_properties(context.physical_device.handle)
        };
        (0..memory_properties.memory_type_count)
            .find(|&i| {
                (type_filter & (1 << i) != 0)
                    && (memory_properties.memory_types[i as usize].property_flags & properties)
                        == properties
            })
            .ok_or_else(|| anyhow::anyhow!("Aucun type mémoire compatible trouvé"))
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

    pub fn bind(&self, command_buffer: vk::CommandBuffer) {
        unsafe {
            self.context.device.cmd_bind_vertex_buffers2(
                command_buffer, // le command buffer en cours d'enregistrement
                0,              // first_binding : index du premier binding (slot 0)
                &[self.buffer], // buffers : liste des buffers à binder
                &[0], // offsets : offset en bytes dans chaque buffer (0 = depuis le début)
                None, // sizes : taille à lire dans chaque buffer (None = jusqu'à la fin)
                None, // strides : override du stride défini dans le pipeline (None = utilise celui du pipeline)
            );
        }
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
