use crate::app::engine::rendering_context::RenderingContext;
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
    free: Vec<FreeRange>,
}

impl MemoryBlock {
    fn new(memory: vk::DeviceMemory, size: u64) -> Self {
        Self {
            memory,
            size,
            free: vec![FreeRange {
                start: 0,
                end: size,
            }],
        }
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

pub struct GpuAllocator {
    blocks: HashMap<u32, Vec<MemoryBlock>>,
}

impl GpuAllocator {
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
    ) -> Option<Allocation> {
        let memory_type = context.find_memory_type(req.memory_type_bits, props);
        None
    }
}
