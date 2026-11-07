use anyhow::Result;
use glam::IVec2;
use std::{
    cmp::Ordering as CmpOrdering,
    collections::BinaryHeap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};

use crate::{
    app::engine::{
        renderer::render_resources::{MeshData, TerrainMesh},
        rendering_context::RenderingContext,
        terrain_generation::{
            LodFocus, LodGrid, MeshCache, MeshKey, ShownLods, TerrainSnapshot, chunk_distance,
            mesh_chunk,
        },
    },
    profile,
};

/// Un chunk absent est un trou à l'écran : il passe avant un chunk à raffiner situé jusqu'à
/// ce facteur plus près. Pas de priorité absolue : la frange lointaine, qui arrive sans
/// cesse en mouvement, affamerait le raffinement proche.
const HOLE_PRIORITY: f32 = 4.0;

/// Chunks publiés au plus : les plus urgents. La cible change au plus toutes les 100 ms, et
/// les workers traitent ~2 300 chunks dans ce temps ; une liste épuisée est republiée
/// aussitôt.
const MAX_PUBLISHED: usize = 4096;

/// **File de maillage** : rapproche ce qui est affiché ([`ShownLods`]) de la configuration
/// visée, chunk par chunk.
///
/// Le mesh d'un chunk ne dépend pas de ses voisins : chacun s'installe seul, dès que ses
/// voisins affichés l'autorisent ([`ShownLods::next_lod`]). Un chunk loin de sa cible y va
/// donc par paliers, le détail arrivant par vagues.
///
/// ```text
/// 1. publier    → liste figée (coord, lod) consommée par les workers rayon
/// 2. récolter   → résultats vers le cache et `ready`
/// 3. construire → buffers Vulkan, du plus proche au plus lointain, sous budget de temps
/// ```
/// La liste est remplacée dès que la cible change : le travail périmé s'arrête au chunk en
/// cours de chaque worker, et ce qui est déjà maillé reste au cache.
pub struct MeshQueue {
    /// Configuration visée.
    target: Arc<LodGrid>,
    /// Point focal de la cible : fixe les priorités.
    focus: LodFocus,
    /// Relief et édits passés aux workers.
    snapshot: TerrainSnapshot,
    sender: Sender<Meshed>,
    receiver: Receiver<Meshed>,
    /// Liste en cours de consommation.
    list: Arc<WorkList>,
    /// Résultats de cette liste pas encore reçus.
    remaining: usize,
    /// Numéro de la liste en cours : distingue les résultats en retard des listes annulées.
    generation: u64,
    /// Géométries prêtes à construire ; le plus proche sort en premier.
    ready: BinaryHeap<Ready>,
    /// La cible a changé depuis la dernière publication.
    target_changed: bool,
    /// Plus rien à faire tant que la cible ne change pas.
    settled: bool,
    /// Temps de construction de buffers autorisé par frame.
    budget: Duration,
    stats: QueueStats,
}

/// Liste figée partagée avec les workers.
struct WorkList {
    items: Vec<(IVec2, u8)>,
    next: AtomicUsize,
    cancelled: AtomicBool,
}

/// Résultat d'un worker.
struct Meshed {
    generation: u64,
    coord: IVec2,
    lod: u8,
    data: MeshData,
    time: Duration,
}

/// Géométrie en attente de ses buffers. Ordonnée par rang **inversé** : `BinaryHeap` rend
/// le plus grand élément, donc ici le rang le plus faible.
struct Ready {
    rank: f32,
    coord: IVec2,
    lod: u8,
    data: Arc<MeshData>,
}

impl PartialEq for Ready {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Ready {}
impl PartialOrd for Ready {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl Ord for Ready {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        other.rank.total_cmp(&self.rank)
    }
}

/// Compteurs depuis le dernier bilan.
#[derive(Default)]
struct QueueStats {
    installed: usize,
    reused: usize,
    /// Par LOD : chunks maillés et temps CPU cumulé. Agrandi à la demande.
    mesh_cost: Vec<(u32, Duration)>,
}

impl MeshQueue {
    pub(super) fn new(
        target: Arc<LodGrid>,
        focus: LodFocus,
        snapshot: TerrainSnapshot,
        budget: Duration,
    ) -> MeshQueue {
        let (sender, receiver) = channel();
        MeshQueue {
            target,
            focus,
            snapshot,
            sender,
            receiver,
            list: Arc::new(WorkList {
                items: Vec::new(),
                next: AtomicUsize::new(0),
                cancelled: AtomicBool::new(false),
            }),
            remaining: 0,
            generation: 0,
            ready: BinaryHeap::new(),
            target_changed: true,
            settled: false,
            budget,
            stats: QueueStats::default(),
        }
    }

    /// Nouvelle configuration visée : la liste en cours sera remplacée.
    pub(super) fn retarget(
        &mut self,
        target: Arc<LodGrid>,
        focus: LodFocus,
        snapshot: TerrainSnapshot,
    ) {
        self.target = target;
        self.focus = focus;
        self.snapshot = snapshot;
        self.target_changed = true;
        self.settled = false;
    }

    /// Temps de construction de buffers autorisé par frame.
    pub(super) fn set_budget(&mut self, budget: Duration) {
        self.budget = budget;
    }

    /// Tout est affiché à son niveau cible.
    pub(super) fn is_settled(&self) -> bool {
        self.settled
    }

    /// Une frame de travail : récolte, publication si besoin, construction. Rend les chunks
    /// construits (`None` = chunk vide) ; `shown` est déjà à jour pour eux.
    pub(super) fn advance(
        &mut self,
        shown: &mut ShownLods,
        cache: &mut MeshCache,
        context: &Arc<RenderingContext>,
    ) -> Result<Vec<(IVec2, Option<TerrainMesh>)>> {
        self.collect(shown, cache);
        let exhausted = !self.settled && self.remaining == 0 && self.ready.is_empty();
        if exhausted || self.target_changed {
            self.publish(shown, cache);
        }
        self.build(shown, context)
    }

    /// Ramasse les résultats des workers. Tout va au cache ; seuls ceux de la liste en
    /// cours sont à construire.
    fn collect(&mut self, shown: &ShownLods, cache: &mut MeshCache) {
        while let Ok(meshed) = self.receiver.try_recv() {
            let lod = meshed.lod as usize;
            if lod >= self.stats.mesh_cost.len() {
                self.stats.mesh_cost.resize(lod + 1, (0, Duration::ZERO));
            }
            let cost = &mut self.stats.mesh_cost[lod];
            cost.0 += 1;
            cost.1 += meshed.time;

            let data = Arc::new(meshed.data);
            let key = MeshKey {
                coord: meshed.coord,
                lod: meshed.lod,
            };
            cache.insert(key, data.clone());
            if meshed.generation == self.generation {
                self.remaining -= 1;
                let rank = self.rank(meshed.coord, shown.get(meshed.coord).is_none());
                self.push_ready(rank, meshed.coord, meshed.lod, data);
            }
        }
    }

    /// Remplace la liste des workers : pour les chunks les plus urgents, le niveau que leurs
    /// voisins autorisent, par rang croissant. Le cache évite le maillage.
    fn publish(&mut self, shown: &ShownLods, cache: &mut MeshCache) {
        profile!();
        self.list.cancelled.store(true, Ordering::Relaxed);
        self.target_changed = false;
        self.generation += 1;
        self.ready.clear();

        let mut candidates = Vec::new();
        let grid = self.target.clone();
        for &coord in grid.coords() {
            let current = shown.get(coord);
            let target = grid.lod(coord);
            if current == Some(target) {
                continue;
            }
            // `None`, ou le niveau courant : bloqué pour l'instant par ses voisins.
            let Some(lod) = shown.next_lod(coord, target) else {
                continue;
            };
            if current == Some(lod) {
                continue;
            }
            candidates.push((self.rank(coord, current.is_none()), coord, lod));
        }
        let by_rank = |a: &(f32, IVec2, u8), b: &(f32, IVec2, u8)| a.0.total_cmp(&b.0);
        if candidates.len() > MAX_PUBLISHED {
            candidates.select_nth_unstable_by(MAX_PUBLISHED, by_rank);
            candidates.truncate(MAX_PUBLISHED);
        }
        candidates.sort_unstable_by(by_rank);
        self.settled = candidates.is_empty();

        let mut items = Vec::with_capacity(candidates.len());
        for (rank, coord, lod) in candidates {
            match cache.get(MeshKey { coord, lod }) {
                Some(data) => {
                    self.stats.reused += 1;
                    self.push_ready(rank, coord, lod, data);
                }
                None => items.push((coord, lod)),
            }
        }

        self.remaining = items.len();
        self.list = Arc::new(WorkList {
            items,
            next: AtomicUsize::new(0),
            cancelled: AtomicBool::new(false),
        });
        if self.remaining > 0 {
            dispatch(
                self.list.clone(),
                self.generation,
                self.snapshot.clone(),
                self.sender.clone(),
            );
        }
    }

    /// Bâtit les buffers des géométries prêtes, du plus proche au plus lointain, **sous
    /// budget de temps**. Un chunk que ses voisins n'autorisent plus reste au cache : la
    /// prochaine publication le reprendra.
    ///
    /// `shown` est mis à jour à chaque chunk : deux voisins construits dans la même frame
    /// doivent respecter la règle l'un envers l'autre.
    fn build(
        &mut self,
        shown: &mut ShownLods,
        context: &Arc<RenderingContext>,
    ) -> Result<Vec<(IVec2, Option<TerrainMesh>)>> {
        profile!();
        let deadline = Instant::now() + self.budget;
        let mut built = Vec::new();

        while let Some(item) = self.ready.pop() {
            if !self.target.contains(item.coord) || shown.get(item.coord) == Some(item.lod) {
                continue;
            }
            let target = self.target.lod(item.coord);
            if shown.next_lod(item.coord, target) != Some(item.lod) {
                continue;
            }
            // Vulkan interdit un buffer de taille 0.
            let mesh = if item.data.is_empty() {
                None
            } else {
                Some(TerrainMesh::new(context.clone(), item.data)?)
            };
            shown.set(item.coord, item.lod);
            built.push((item.coord, mesh));
            self.stats.installed += 1;

            // Test après avoir traité un élément : au moins un par frame, toujours.
            if Instant::now() >= deadline {
                break;
            }
        }
        Ok(built)
    }

    /// Rang de traitement de `coord` : plus il est faible, plus le chunk est urgent.
    fn rank(&self, coord: IVec2, missing: bool) -> f32 {
        let distance = chunk_distance(coord, self.focus.pos);
        if missing { distance / HOLE_PRIORITY } else { distance }
    }

    fn push_ready(&mut self, rank: f32, coord: IVec2, lod: u8, data: Arc<MeshData>) {
        self.ready.push(Ready {
            rank,
            coord,
            lod,
            data,
        });
    }

    /// Bilan depuis le dernier appel, `None` si rien n'a été installé.
    pub(super) fn take_report(&mut self) -> Option<String> {
        if self.stats.installed == 0 {
            return None;
        }
        let stats = std::mem::take(&mut self.stats);
        let per_lod: Vec<String> = stats
            .mesh_cost
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| *n > 0)
            .map(|(lod, (n, t))| format!("L{lod} {} µs ×{n}", t.as_micros() / *n as u128))
            .collect();
        let meshed: u32 = stats.mesh_cost.iter().map(|(n, _)| n).sum();
        Some(format!(
            "{} chunks installés ({meshed} maillés, {} repris du cache) [{}]",
            stats.installed,
            stats.reused,
            per_lod.join(", "),
        ))
    }
}

/// Lance le maillage de `list` sur tous les threads du pool, **dans l'ordre** de la liste.
///
/// Pas de `par_iter` : son découpage récursif fait voler aux threads inactifs la plus
/// grosse moitié restante, donc la fin de la liste démarre presque tout de suite. Ici
/// chaque thread prend le prochain index d'un compteur atomique partagé — une file.
/// `spawn_broadcast` rend la main aussitôt : le jeu continue de tourner pendant ce temps.
fn dispatch(
    list: Arc<WorkList>,
    generation: u64,
    snapshot: TerrainSnapshot,
    sender: Sender<Meshed>,
) {
    rayon::spawn_broadcast(move |_| {
        while !list.cancelled.load(Ordering::Relaxed) {
            let i = list.next.fetch_add(1, Ordering::Relaxed);
            let Some(&(coord, lod)) = list.items.get(i) else {
                return;
            };
            let start = Instant::now();
            let data = mesh_chunk(&snapshot, lod, coord);
            let meshed = Meshed {
                generation,
                coord,
                lod,
                data,
                time: start.elapsed(),
            };
            // Récepteur détruit (terrain régénéré entre-temps) : la liste ne sert plus à rien.
            if sender.send(meshed).is_err() {
                return;
            }
        }
    });
}
