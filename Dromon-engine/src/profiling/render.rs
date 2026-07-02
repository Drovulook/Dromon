//! Profiling GPU par *timestamp queries*.
//!
//! Le GPU étant asynchrone, on ne le chronomètre pas côté CPU : on insère des
//! `vkCmdWriteTimestamp` dans le command buffer, que le GPU horodate quand il les
//! atteint. On lit les résultats 2 frames plus tard (après le fence du slot) :
//! `(end - begin) × timestamp_period` = durée réelle GPU. Envoi via `[PROFILE_GPU]`,
//! throttlé à 500 ms. Vit dans le `Renderer` (a besoin du `device`), contrairement au
//! profiling CPU qui est global.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ash::vk;

use crate::app::logger::Logger;

const MAX_QUERIES_PER_SLOT: u32 = 64; // 32 scopes max par frame
const FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// Une région mesurée : paire de queries (begin, end) + place dans l'arbre.
struct Region {
    name: &'static str,
    depth: usize,
    begin: u32,
    end: u32,
}

/// État d'enregistrement d'un slot in-flight, remis à zéro à chaque cycle.
struct SlotState {
    regions: Vec<Region>,
    next_query: u32,
    depth: usize,
}

pub struct GpuProfiler {
    device: ash::Device,
    logger: Arc<Logger>,
    pools: Vec<vk::QueryPool>, // un par slot in-flight
    ns_per_tick: f64,
    supported: bool,
    slots: RefCell<Vec<SlotState>>,
    last_flush: Cell<Option<Instant>>,
}

impl GpuProfiler {
    /// Crée les query pools (un par slot). No-op si le profiling est coupé ou si la
    /// queue ne supporte pas les timestamps → toutes les méthodes deviennent inertes.
    pub fn new(
        device: ash::Device,
        logger: Arc<Logger>,
        timestamp_period: f32,
        timestamp_valid_bits: u32,
        in_flight_count: usize,
    ) -> anyhow::Result<Self> {
        let supported = super::enabled() && timestamp_period != 0.0 && timestamp_valid_bits > 0;
        if !supported {
            return Ok(Self {
                device,
                logger,
                pools: Vec::new(),
                ns_per_tick: 0.0,
                supported: false,
                slots: RefCell::new(Vec::new()),
                last_flush: Cell::new(None),
            });
        }

        let mut pools = Vec::with_capacity(in_flight_count);
        let mut slots = Vec::with_capacity(in_flight_count);
        for _ in 0..in_flight_count {
            let pool = unsafe {
                device.create_query_pool(
                    &vk::QueryPoolCreateInfo::default()
                        .query_type(vk::QueryType::TIMESTAMP)
                        .query_count(MAX_QUERIES_PER_SLOT),
                    None,
                )?
            };
            pools.push(pool);
            slots.push(SlotState {
                regions: Vec::new(),
                next_query: 0,
                depth: 0,
            });
        }

        Ok(Self {
            device,
            logger,
            pools,
            ns_per_tick: timestamp_period as f64,
            supported: true,
            slots: RefCell::new(slots),
            last_flush: Cell::new(None),
        })
    }

    /// À enregistrer en tout début de command buffer (hors rendering scope) : remet à
    /// zéro le pool du slot et son état d'enregistrement.
    pub fn reset(&self, cmd: vk::CommandBuffer, slot: usize) {
        if !self.supported {
            return;
        }
        unsafe {
            self.device
                .cmd_reset_query_pool(cmd, self.pools[slot], 0, MAX_QUERIES_PER_SLOT);
        }
        let mut slots = self.slots.borrow_mut();
        let s = &mut slots[slot];
        s.regions.clear();
        s.next_query = 0;
        s.depth = 0;
    }

    /// Ouvre une région : écrit le timestamp de début et renvoie un garde qui écrit
    /// celui de fin à son `Drop`. `None` si non supporté ou pool plein.
    pub fn scope(
        &self,
        cmd: vk::CommandBuffer,
        slot: usize,
        name: &'static str,
    ) -> Option<GpuScope<'_>> {
        if !self.supported {
            return None;
        }
        let (begin, end) = {
            let mut slots = self.slots.borrow_mut();
            let s = &mut slots[slot];
            if s.next_query + 2 > MAX_QUERIES_PER_SLOT {
                return None;
            }
            let (begin, end) = (s.next_query, s.next_query + 1);
            s.next_query += 2;
            s.regions.push(Region {
                name,
                depth: s.depth,
                begin,
                end,
            });
            s.depth += 1;
            (begin, end)
        };
        self.write(cmd, slot, begin);
        Some(GpuScope {
            profiler: self,
            cmd,
            slot,
            end,
        })
    }

    fn write(&self, cmd: vk::CommandBuffer, slot: usize, query: u32) {
        unsafe {
            self.device.cmd_write_timestamp(
                cmd,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                self.pools[slot],
                query,
            );
        }
    }

    /// Lit les résultats du slot (frame précédente, complète car le fence est passé),
    /// calcule les durées et les envoie — au plus une fois toutes les 500 ms.
    pub fn resolve(&self, slot: usize) {
        if !self.supported {
            return;
        }
        let now = Instant::now();
        let due = match self.last_flush.get() {
            None => true,
            Some(t) => now.duration_since(t) >= FLUSH_INTERVAL,
        };
        if !due {
            return;
        }

        let slots = self.slots.borrow();
        let s = &slots[slot];
        if s.next_query == 0 {
            return; // rien enregistré pour ce slot pour l'instant
        }

        let mut data = vec![0u64; s.next_query as usize];
        if unsafe {
            self.device.get_query_pool_results(
                self.pools[slot],
                0,
                &mut data,
                vk::QueryResultFlags::TYPE_64,
            )
        }
        .is_err()
        {
            return;
        }

        let records: Vec<String> = s
            .regions
            .iter()
            .map(|r| {
                let ticks = data[r.end as usize].wrapping_sub(data[r.begin as usize]);
                let dur_us = (ticks as f64 * self.ns_per_tick / 1000.0) as u128;
                format!("{}\u{1f}{}\u{1f}1\u{1f}{}", r.depth, dur_us, r.name)
            })
            .collect();
        if !records.is_empty() {
            self.logger.profile("GPU", &records.join("\u{1e}"));
        }
        self.last_flush.set(Some(now));
    }

    /// À appeler dans le `Drop` du renderer, après `device_wait_idle`.
    pub fn destroy(&self) {
        for &pool in &self.pools {
            unsafe {
                self.device.destroy_query_pool(pool, None);
            }
        }
    }
}

/// Garde RAII d'une région GPU : son `Drop` écrit le timestamp de fin.
pub struct GpuScope<'a> {
    profiler: &'a GpuProfiler,
    cmd: vk::CommandBuffer,
    slot: usize,
    end: u32,
}

impl Drop for GpuScope<'_> {
    fn drop(&mut self) {
        self.profiler.write(self.cmd, self.slot, self.end);
        self.profiler.slots.borrow_mut()[self.slot].depth -= 1;
    }
}
