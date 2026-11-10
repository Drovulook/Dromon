//! **Ring buffer de staging** : un seul buffer host-visible, réutilisé en boucle, pour
//! tous les envois CPU → arènes. Une zone écrite à la frame `f` n'est réutilisée qu'à
//! `f + frames_in_flight + 1`, quand sa copie est sûrement exécutée (même délai que
//! le cimetière du terrain).

use crate::app::engine::renderer::buffer::Buffer;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::Result;
use ash::vk;
use std::collections::VecDeque;
use std::sync::Arc;

/// Alignement des réservations : garde le `memcpy` sur des adresses alignées.
const ALIGN: u64 = 16;

pub struct StagingRing {
    buffer: Buffer,
    capacity: u64,
    /// Positions **absolues**, jamais ramenées modulo `capacity` : `head - tail` est la
    /// place occupée, sans ambiguïté entre ring plein et ring vide.
    head: u64,
    tail: u64,
    /// `head` au début de la frame courante, pour le budget.
    frame_start: u64,
    /// Octets réservés par frame, au-delà de la première réservation.
    frame_budget: u64,
    /// `head` à la fin de chacune des dernières frames, les plus anciennes en tête.
    frame_ends: VecDeque<u64>,
    /// Frames à attendre avant de réutiliser une zone.
    guard: usize,
}

impl StagingRing {
    /// Ring de `frame_budget × (frames_in_flight + 1)` octets : de quoi tenir toutes les
    /// frames dont la copie peut encore être en cours.
    pub fn new(
        context: Arc<RenderingContext>,
        frame_budget: u64,
        frames_in_flight: u64,
    ) -> Result<Self> {
        let guard = frames_in_flight as usize + 1;
        let capacity = frame_budget * guard as u64;
        let buffer = Buffer::new(
            context,
            capacity,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        Ok(Self {
            buffer,
            capacity,
            head: 0,
            tail: 0,
            frame_start: 0,
            frame_budget,
            frame_ends: VecDeque::new(),
            guard,
        })
    }

    /// Source des `cmd_copy_buffer`.
    pub fn buffer(&self) -> vk::Buffer {
        self.buffer.buffer
    }

    /// Octets réservés par frame : la taille du ring en dépend.
    pub fn frame_budget(&self) -> u64 {
        self.frame_budget
    }

    /// Oublie toutes les réservations. ⚠ Le GPU doit être à l'arrêt : aucune copie
    /// depuis le ring ne doit rester à exécuter.
    pub fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.frame_start = 0;
        self.frame_ends.clear();
    }

    /// À appeler une fois par frame **soumise**, après l'enregistrement de ses copies :
    /// clôt la frame et recycle la zone de la frame `courante - guard`. Une frame sautée
    /// (fenêtre minimisée) n'appelle pas : ses écritures attendent leur copie, et le
    /// budget déjà consommé bloque les suivantes.
    pub fn end_frame(&mut self) {
        self.frame_ends.push_back(self.head);
        while self.frame_ends.len() >= self.guard {
            if let Some(end) = self.frame_ends.pop_front() {
                self.tail = end;
            }
        }
        self.frame_start = self.head;
    }

    /// Réserve `bytes` d'un seul tenant et rend leur offset dans le buffer. `None` : ring
    /// plein ou budget de la frame atteint, à retenter à la frame suivante. La première
    /// réservation d'une frame ignore le budget, pour qu'un gros envoi passe toujours.
    pub fn allocate(&mut self, bytes: u64) -> Option<u64> {
        debug_assert!(bytes > 0);
        let size = (bytes + ALIGN - 1) & !(ALIGN - 1);
        // Une réservation n'est jamais coupée par la fin du buffer : on saute le reste.
        let pos = self.head % self.capacity;
        let skip = if pos + size > self.capacity {
            self.capacity - pos
        } else {
            0
        };
        let end = self.head + skip + size;
        let over_budget = self.head != self.frame_start
            && end - self.frame_start > self.frame_budget;
        if end - self.tail > self.capacity || over_budget {
            return None;
        }
        let offset = (self.head + skip) % self.capacity;
        self.head = end;
        Some(offset)
    }

    /// Copie `data` à `offset`, dans une zone rendue par [`StagingRing::allocate`].
    pub fn write<T: Copy>(&self, offset: u64, data: &[T]) -> Result<()> {
        let bytes = std::mem::size_of_val(data);
        debug_assert!(offset + bytes as u64 <= self.capacity);
        let base = self.buffer.map()?;
        unsafe {
            // Copie octet par octet, comme `Buffer::upload` : rien ne garantit que
            // `base + offset` soit aligné pour `T`.
            std::ptr::copy_nonoverlapping(
                data.as_ptr() as *const u8,
                base.add(offset as usize),
                bytes,
            );
        }
        Ok(())
    }
}
