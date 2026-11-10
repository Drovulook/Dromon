//! Mémoire GPU : blocs demandés au driver (`device_memory`), découpage générique
//! d'un intervalle en tranches (`suballocator`), gros buffers partagés (`arena`), envois
//! CPU → GPU (`staging_ring`).

pub(crate) mod arena;
pub(crate) mod device_memory;
pub(crate) mod staging_ring;
mod suballocator;
