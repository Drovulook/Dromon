use crate::app::engine::rendering_context::RenderingContext;
use anyhow::{Context, Result};
use ash::vk;
use std::collections::HashMap;

#[derive(Clone, Copy)]
struct FreeRange {
    start: u64,
    end: u64,
}

pub struct MemoryBlock {
    memory: vk::DeviceMemory,
    size: u64,
    mapped: Option<*mut u8>,
    free: Vec<FreeRange>,
}
/// Taille d'un bloc demandé au driver. Un mesh LOD0 fait ~2 Mo : un bloc en tient
/// une trentaine, et le monde entier quelques blocs au lieu de ~31 000 allocations.
const BLOCK_SIZE: u64 = 64 * 1024 * 1024;

impl MemoryBlock {
    fn new(context: &RenderingContext, memory_type: u32, size: u64) -> Result<Self> {
        let memory = unsafe {
            context.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(size)
                    .memory_type_index(memory_type),
                None,
            )?
        };

        let host_visible = context.physical_device.memory_properties.memory_types
            [memory_type as usize]
            .property_flags
            .contains(vk::MemoryPropertyFlags::HOST_VISIBLE);
        let mapped = if host_visible {
            let ptr = unsafe {
                context
                    .device
                    .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())?
            };
            Some(ptr as *mut u8)
        } else {
            None
        };

        Ok(Self {
            memory,
            size,
            mapped,
            free: vec![FreeRange {
                start: 0,
                end: size,
            }],
        })
    }

    /// surveiller la fragmentation
    fn free_bytes(&self) -> u64 {
        self.free.iter().map(|r| r.end - r.start).sum()
    }

    /// Le masque suppose `a` puissance de deux — ce que la spec Vulkan garantit pour
    /// `MemoryRequirements::alignment`.
    fn align_up(x: u64, a: u64) -> u64 {
        debug_assert!(a.is_power_of_two());
        (x + a - 1) & !(a - 1)
    }

    fn try_alloc(&mut self, size: u64, align: u64) -> Option<u64> {
        // Une tranche vide passerait tous les tests et fausserait la libération.
        debug_assert!(size > 0);
        for i in 0..self.free.len() {
            let hole = self.free[i];
            let start = Self::align_up(hole.start, align);
            if start <= hole.end && hole.end - start >= size {
                // résidu à l'arrière
                if start + size < hole.end {
                    self.free.insert(
                        i + 1,
                        FreeRange {
                            start: start + size,
                            end: hole.end,
                        },
                    );
                }
                // résidu à l'avant
                if start == hole.start {
                    self.free.remove(i);
                } else {
                    self.free[i].end = start;
                }
                return Some(start);
            }
        }
        None
    }

    /// Rend la tranche `[offset, offset + size)` et refusionne avec ses voisins
    /// immédiats.
    fn dealloc(&mut self, offset: u64, size: u64) {
        let range = FreeRange {
            start: offset,
            end: offset + size,
        };
        let i = self.free.partition_point(|r| r.start < offset);
        // il ne doit pas y avoir de chevauchement
        debug_assert!(i == 0 || self.free[i - 1].end <= range.start);
        debug_assert!(i == self.free.len() || range.end <= self.free[i].start);
        self.free.insert(i, range);

        if i + 1 < self.free.len() && self.free[i].end == self.free[i + 1].start {
            self.free[i].end = self.free[i + 1].end;
            self.free.remove(i + 1);
        }
        if i > 0 && self.free[i - 1].end == self.free[i].start {
            self.free[i - 1].end = self.free[i].end;
            self.free.remove(i);
        }
    }
}

pub struct Allocation {
    pub memory: vk::DeviceMemory, // → bind_buffer_memory
    pub offset: u64,              // → bind_buffer_memory
    pub size: u64,
    block: usize, // index dans blocks[memory_type], pour libérer
    memory_type: u32,
}

pub struct DeviceMemoryAllocator {
    blocks: HashMap<u32, Vec<MemoryBlock>>,
}

impl DeviceMemoryAllocator {
    pub fn new() -> Self {
        Self {
            blocks: HashMap::new(),
        }
    }

    pub fn allocate(
        &mut self,
        context: &RenderingContext,
        req: vk::MemoryRequirements,
        props: vk::MemoryPropertyFlags,
    ) -> Result<Allocation> {
        let memory_type = context.find_memory_type(req.memory_type_bits, props)?;
        let blocks = self.blocks.entry(memory_type).or_default();

        // First-fit sur les blocs existants. `enumerate` parce que l'index du bloc doit
        // repartir dans l'`Allocation` : c'est lui qui permettra de la rendre.
        let hit = blocks.iter_mut().enumerate().find_map(|(i, block)| {
            block
                .try_alloc(req.size, req.alignment)
                .map(|offset| (i, offset))
        });

        let (block, offset) = match hit {
            Some(hit) => hit,
            None => {
                // Repli sur `req.size` : une demande plus grosse qu'un bloc standard
                // (grosse texture) obtient un bloc à sa mesure plutôt qu'un échec.
                let size = BLOCK_SIZE.max(req.size);
                blocks.push(MemoryBlock::new(context, memory_type, size)?);
                let index = blocks.len() - 1;
                let offset = blocks[index]
                    .try_alloc(req.size, req.alignment)
                    .context("un bloc neuf doit servir la demande qui l'a créé")?;
                (index, offset)
            }
        };

        Ok(Allocation {
            memory: blocks[block].memory,
            offset,
            size: req.size,
            block,
            memory_type,
        })
    }

    pub fn free_alloc(&mut self, allocation: &Allocation) {
        let block = self
            .blocks
            .get_mut(&allocation.memory_type)
            .and_then(|blocks| blocks.get_mut(allocation.block));
        debug_assert!(block.is_some(), "allocation étrangère à cet allocateur");
        if let Some(block) = block {
            block.dealloc(allocation.offset, allocation.size);
        }
    }

    pub fn destroy_allocator(&mut self, context: &RenderingContext) {
        for (_, blocks) in self.blocks.drain() {
            for block in blocks {
                unsafe {
                    context.device.free_memory(block.memory, None);
                }
            }
        }
    }
}
