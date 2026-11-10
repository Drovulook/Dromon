//! Découpe d'un intervalle `[0, capacité)` en tranches : best-fit, refusion des trous
//! voisins, tout en O(log n) grâce à deux index triés des trous. Sans Vulkan ni unité
//! imposée : octets pour `DeviceMemoryAllocator`, éléments pour les arènes.

use std::collections::{BTreeMap, BTreeSet};

pub struct Suballocator {
    capacity: u64,
    /// Trous par position : `début → fin`. Jamais deux adjacents (ils sont refusionnés).
    by_start: BTreeMap<u64, u64>,
    /// Les mêmes trous par `(taille, début)` : le premier ≥ `(n, 0)` est le plus petit
    /// trou d'au moins `n`.
    by_size: BTreeSet<(u64, u64)>,
    free_total: u64,
}

impl Suballocator {
    pub fn new(capacity: u64) -> Self {
        let mut ranges = Self {
            capacity,
            by_start: BTreeMap::new(),
            by_size: BTreeSet::new(),
            free_total: 0,
        };
        if capacity > 0 {
            ranges.insert_hole(0, capacity);
        }
        ranges
    }

    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    pub fn free_total(&self) -> u64 {
        self.free_total
    }

    pub fn holes(&self) -> usize {
        self.by_start.len()
    }

    /// Plus grande tranche allouable d'un seul tenant.
    pub fn largest_free(&self) -> u64 {
        self.by_size.last().map_or(0, |&(size, _)| size)
    }

    /// Le masque suppose `a` puissance de deux — ce que la spec Vulkan garantit pour
    /// `MemoryRequirements::alignment`.
    fn align_up(x: u64, a: u64) -> u64 {
        debug_assert!(a.is_power_of_two());
        (x + a - 1) & !(a - 1)
    }

    /// Les deux index changent toujours ensemble : seules ces deux fonctions y touchent.
    fn insert_hole(&mut self, start: u64, end: u64) {
        self.by_start.insert(start, end);
        self.by_size.insert((end - start, start));
        self.free_total += end - start;
    }

    fn remove_hole(&mut self, start: u64, end: u64) {
        self.by_start.remove(&start);
        self.by_size.remove(&(end - start, start));
        self.free_total -= end - start;
    }

    /// Début d'une tranche de `size` alignée sur `align`, ou `None` si aucun trou ne
    /// convient. Best-fit : le plus petit trou qui tient, pour garder les grands intacts.
    pub fn try_alloc(&mut self, size: u64, align: u64) -> Option<u64> {
        // Une tranche vide passerait tous les tests et fausserait la libération.
        debug_assert!(size > 0);
        // Avec `align` > 1, un trou assez grand peut ne plus tenir une fois son début
        // arrondi : on passe au suivant (rare, et jamais pour les arènes).
        let (hole_start, hole_end, start) =
            self.by_size
                .range((size, 0)..)
                .find_map(|&(hole_size, hole_start)| {
                    let hole_end = hole_start + hole_size;
                    let start = Self::align_up(hole_start, align);
                    (start + size <= hole_end).then_some((hole_start, hole_end, start))
                })?;
        self.remove_hole(hole_start, hole_end);
        // résidus avant (dû à l'alignement) et après
        if hole_start < start {
            self.insert_hole(hole_start, start);
        }
        if start + size < hole_end {
            self.insert_hole(start + size, hole_end);
        }
        Some(start)
    }

    /// Rend la tranche `[offset, offset + size)` et refusionne avec ses voisins
    /// immédiats.
    pub fn dealloc(&mut self, offset: u64, size: u64) {
        let mut start = offset;
        let mut end = offset + size;
        // Copiés hors des index : `remove_hole` les modifie ensuite.
        let prev = self
            .by_start
            .range(..offset)
            .next_back()
            .map(|(&s, &e)| (s, e));
        let next = self.by_start.range(offset..).next().map(|(&s, &e)| (s, e));
        // il ne doit pas y avoir de chevauchement
        debug_assert!(prev.is_none_or(|(_, e)| e <= offset));
        debug_assert!(next.is_none_or(|(s, _)| end <= s));

        if let Some((s, e)) = prev.filter(|&(_, e)| e == offset) {
            self.remove_hole(s, e);
            start = s;
        }
        if let Some((s, e)) = next.filter(|&(s, _)| s == end) {
            self.remove_hole(s, e);
            end = e;
        }
        self.insert_hole(start, end);
    }
}
