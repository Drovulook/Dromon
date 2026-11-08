//! Découpe d'un intervalle `[0, capacité)` en tranches : free-list triée, first-fit,
//! refusion des trous voisins. Sans Vulkan ni unité imposée : octets pour
//! `DeviceMemoryAllocator`, éléments pour les arènes.

#[derive(Clone, Copy)]
struct FreeRange {
    start: u64,
    end: u64,
}

pub struct Suballocator {
    capacity: u64,
    /// Trous triés par `start`, jamais deux adjacents : invariant dont dépendent
    /// `try_alloc` et `dealloc`.
    free: Vec<FreeRange>,
}

impl Suballocator {
    pub fn new(capacity: u64) -> Self {
        Self {
            capacity,
            free: vec![FreeRange {
                start: 0,
                end: capacity,
            }],
        }
    }

    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    pub fn free_total(&self) -> u64 {
        self.free.iter().map(|r| r.end - r.start).sum()
    }

    pub fn holes(&self) -> usize {
        self.free.len()
    }

    /// Plus grande tranche allouable d'un seul tenant.
    pub fn largest_free(&self) -> u64 {
        self.free.iter().map(|r| r.end - r.start).max().unwrap_or(0)
    }

    /// Le masque suppose `a` puissance de deux — ce que la spec Vulkan garantit pour
    /// `MemoryRequirements::alignment`.
    fn align_up(x: u64, a: u64) -> u64 {
        debug_assert!(a.is_power_of_two());
        (x + a - 1) & !(a - 1)
    }

    /// Début d'une tranche de `size` alignée sur `align`, ou `None` si aucun trou ne
    /// convient.
    pub fn try_alloc(&mut self, size: u64, align: u64) -> Option<u64> {
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
    pub fn dealloc(&mut self, offset: u64, size: u64) {
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
