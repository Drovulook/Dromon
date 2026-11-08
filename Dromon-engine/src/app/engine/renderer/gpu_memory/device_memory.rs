use super::suballocator::Suballocator;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::{Context, Result};
use ash::vk;
use std::collections::HashMap;

/// Base d'un bloc host-visible, mappée à sa création et jamais démappée.
///
/// Un `*mut u8` nu est `!Send`, ce qui suffirait à rendre tout `RenderingContext`
/// `!Sync` et donc `Arc<RenderingContext>` intransportable entre threads.
struct MappedPtr(*mut u8);
unsafe impl Send for MappedPtr {}

struct MemoryBlock {
    memory: vk::DeviceMemory,
    pub mapped: Option<MappedPtr>,
    /// Découpage du bloc, en octets.
    ranges: Suballocator,
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
            Some(MappedPtr(ptr as *mut u8))
        } else {
            None
        };

        Ok(Self {
            memory,
            mapped,
            ranges: Suballocator::new(size),
        })
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

/// Instantané de l'occupation, pour surveiller la **fragmentation externe**.
#[derive(Default)]
pub struct MemoryUsage {
    /// Blocs demandés au driver — c'est ce que plafonne `maxMemoryAllocationCount`.
    pub blocks: usize,
    /// Octets demandés au driver.
    pub reserved: u64,
    /// Octets couverts par des tranches vivantes.
    pub used: u64,
    /// Trous libres, tous blocs confondus.
    pub holes: usize,
    /// Plus grande tranche encore allouable d'un seul tenant.
    pub largest_free: u64,
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
                .ranges
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
                    .ranges
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

    /// Pointeur CPU vers le début de la tranche, `None` si son bloc n'est pas
    /// host-visible. Le mapping appartient au bloc et vit aussi longtemps que lui :
    /// l'appelant ne démappe jamais.
    pub fn mapped_ptr(&self, allocation: &Allocation) -> Option<*mut u8> {
        let block = self
            .blocks
            .get(&allocation.memory_type)?
            .get(allocation.block)?;
        // La tranche est vivante, donc incluse dans le bloc, donc dans le mapping qui
        // le couvre en entier : le pointeur décalé reste dans la même allocation.
        block
            .mapped
            .as_ref()
            .map(|base| unsafe { base.0.add(allocation.offset as usize) })
    }

    pub fn usage(&self) -> MemoryUsage {
        let mut usage = MemoryUsage::default();
        // `values()` donne les `Vec` par memory type, `flatten` les parcourt d'affilée :
        // on agrège tout, staging host-visible et device-local confondus.
        for block in self.blocks.values().flatten() {
            let ranges = &block.ranges;
            usage.blocks += 1;
            usage.reserved += ranges.capacity();
            usage.used += ranges.capacity() - ranges.free_total();
            usage.holes += ranges.holes();
            usage.largest_free = usage.largest_free.max(ranges.largest_free());
        }
        usage
    }

    pub fn free_alloc(&mut self, allocation: &Allocation) {
        let block = self
            .blocks
            .get_mut(&allocation.memory_type)
            .and_then(|blocks| blocks.get_mut(allocation.block));
        debug_assert!(block.is_some(), "allocation étrangère à cet allocateur");
        if let Some(block) = block {
            block.ranges.dealloc(allocation.offset, allocation.size);
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
