//! Mémoire GPU : blocs demandés au driver (`device_memory`), découpage générique
//! d'un intervalle en tranches (`suballocator`), gros buffers partagés (`arena`).

pub(crate) mod arena;
pub(crate) mod device_memory;
mod suballocator;
