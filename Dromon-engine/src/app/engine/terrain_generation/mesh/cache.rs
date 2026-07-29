//! **Cache multi-LOD** : garde en RAM les géométries déjà calculées au lieu de les
//! jeter, pour que les allers-retours de caméra ne repaient pas le maillage.
//!
//! ## Pourquoi côté CPU et pas côté GPU
//! ```text
//! mailler un chunk : 2 000 – 10 000 µs   ← le coût qu'on veut éviter
//! uploader un mesh :        ~50 µs       ← ce qu'on repaie en cachant côté CPU
//! ```
//! Cacher les [`MeshData`] plutôt que les `TerrainMesh` complets élimine ~99 % du coût
//! pour **zéro VRAM**, en RAM système qui est bien plus abondante. Et comme les données
//! vivent derrière un `Arc` partagé avec le mesh affiché, une entrée encore à l'écran ne
//! coûte rien de plus que sans cache.
//!
//! ## Complémentaire de l'hystérésis, pas redondant
//! L'hystérésis absorbe le **bruit** (caméra qui vibre sur une frontière d'anneau) ; le
//! cache absorbe les **allers-retours de moyenne amplitude** — avancer de 300 unités puis
//! revenir, où les deux seuils ont réellement été franchis.
//!
//! ## La clé inclut les coutures
//! `(coord, lod, faces)` et non `(coord, lod)` : la géométrie dépend aussi du
//! rétrécissement demi-pas imposé par les faces de transition. Ressortir une maille aux
//! mauvaises coutures rouvrirait une fissure. Un même `(coord, lod)` peut donc exister en
//! plusieurs variantes, ce qui abaisse un peu le taux de réutilisation.

use crate::app::engine::renderer::render_resources::MeshData;
use crate::app::engine::terrain_generation::lod::TransitionFaces;
use glam::IVec2;
use rustc_hash::FxHashMap;
use std::sync::Arc;

/// Budget mémoire par défaut du cache. Borné en **octets** et non en nombre d'entrées :
/// les tailles sont très inégales (LOD1 ≈ ¼ du LOD0, LOD2 ≈ 6 %, LOD3 ≈ 1,5 %).
pub const DEFAULT_BUDGET_BYTES: usize = 500 * 1024 * 1024;

/// Fraction du budget visée après une éviction : on descend franchement sous la barre
/// plutôt que de re-trier à chaque insertion suivante.
const EVICT_DOWN_TO: f32 = 0.9;

/// Identité d'une géométrie de chunk. Deux mailles de même clé sont interchangeables.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshKey {
    pub coord: IVec2,
    pub lod: u8,
    pub faces: TransitionFaces,
}

struct Entry {
    data: Arc<MeshData>,
    bytes: usize,
    /// Date logique du dernier accès (cf. [`MeshCache::clock`]) — base de l'éviction LRU.
    last_used: u64,
}

/// Cache LRU borné en octets des géométries de chunks.
pub struct MeshCache {
    entries: FxHashMap<MeshKey, Entry>,
    bytes: usize,
    budget: usize,
    /// Horloge logique : incrémentée à chaque accès, sert d'ordre LRU sans avoir à
    /// maintenir une liste chaînée (l'éviction, rare, trie une fois).
    clock: u64,
    /// Plancher sous lequel une nouvelle tentative d'éviction serait vaine (cf.
    /// [`MeshCache::evict`]) : évite de re-trier à chaque insertion quand tout le
    /// dépassement vient d'entrées encore affichées, donc inévinçables.
    evict_floor: usize,
    hits: u64,
    misses: u64,
}

impl MeshCache {
    pub fn new(budget: usize) -> MeshCache {
        MeshCache {
            entries: FxHashMap::default(),
            bytes: 0,
            budget,
            clock: 0,
            evict_floor: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// Géométrie déjà connue pour cette clé, le cas échéant. Compte le hit/miss.
    pub fn get(&mut self, key: MeshKey) -> Option<Arc<MeshData>> {
        self.clock += 1;
        let clock = self.clock;
        match self.entries.get_mut(&key) {
            Some(entry) => {
                entry.last_used = clock;
                self.hits += 1;
                Some(entry.data.clone())
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// Mémorise une géométrie fraîchement maillée. Les chunks **vides** sont cachés aussi
    /// (coût nul, et ça évite de re-mailler pour rien un chunk hors sol).
    pub fn insert(&mut self, key: MeshKey, data: Arc<MeshData>) {
        self.clock += 1;
        let bytes = data.byte_size();
        if let Some(old) = self.entries.insert(
            key,
            Entry {
                data,
                bytes,
                last_used: self.clock,
            },
        ) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
        if self.bytes > self.budget.max(self.evict_floor) {
            self.evict();
        }
    }

    /// Purge toutes les variantes d'un chunk. À appeler quand ses **édits** changent
    /// (creuser/remblayer) : la géométrie mémorisée devient fausse à tous les niveaux.
    ///
    /// Pas encore appelé — l'édition de terrain n'existe pas. C'est le point d'accroche
    /// à ne surtout pas oublier le jour où elle arrivera, sinon creuser laissera le
    /// vieux relief réapparaître au premier changement de LOD.
    #[allow(dead_code)]
    pub fn invalidate(&mut self, coord: IVec2) {
        let mut freed = 0;
        self.entries.retain(|k, e| {
            if k.coord == coord {
                freed += e.bytes;
                return false;
            }
            true
        });
        self.bytes -= freed;
    }

    /// `(hits, misses)` cumulés. La rentabilité du cache dépend entièrement du
    /// déplacement — exploration en ligne droite : ~0 % de réutilisation ; joueur qui
    /// tourne autour d'une base : très rentable. Ces compteurs tranchent en une session
    /// plutôt qu'en supposant.
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    /// Octets actuellement retenus et nombre d'entrées.
    pub fn usage(&self) -> (usize, usize) {
        (self.bytes, self.entries.len())
    }

    /// Évince les entrées les moins récemment utilisées jusqu'à repasser sous
    /// `EVICT_DOWN_TO · budget`. Un tri complet, mais seulement quand le cache déborde.
    ///
    /// ⚠ **Ne compte que les entrées détenues en propre** (`strong_count == 1`). Une
    /// géométrie encore référencée par un `TerrainMesh` affiché occupe la RAM de toute
    /// façon : la retirer du cache ne libère rien et la rendrait juste introuvable au
    /// moment où elle quitte l'écran — exactement le cas qu'on veut couvrir. Le budget
    /// borne donc le **surcoût** du cache, pas la RAM totale du terrain.
    ///
    /// Quand tout le dépassement vient d'entrées partagées, l'éviction ne peut rien : on
    /// remonte alors `evict_floor` pour ne pas re-trier à chaque insertion suivante.
    fn evict(&mut self) {
        let target = (self.budget as f32 * EVICT_DOWN_TO) as usize;
        let mut owned: Vec<(u64, MeshKey, usize)> = self
            .entries
            .iter()
            .filter(|(_, e)| Arc::strong_count(&e.data) == 1)
            .map(|(&k, e)| (e.last_used, k, e.bytes))
            .collect();

        let mut owned_bytes: usize = owned.iter().map(|&(_, _, b)| b).sum();
        if owned_bytes <= target {
            self.evict_floor = self.bytes + self.budget / 10;
            return;
        }
        owned.sort_unstable_by_key(|&(age, _, _)| age);

        for (_, key, bytes) in owned {
            if owned_bytes <= target {
                break;
            }
            if self.entries.remove(&key).is_some() {
                self.bytes -= bytes;
                owned_bytes -= bytes;
            }
        }
        self.evict_floor = 0;
    }
}

impl Default for MeshCache {
    fn default() -> Self {
        MeshCache::new(DEFAULT_BUDGET_BYTES)
    }
}
