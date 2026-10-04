use anyhow::Result;
use glam::IVec2;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, Sender, TryRecvError, channel},
    },
    time::{Duration, Instant},
};

use crate::{
    app::engine::{
        renderer::{
            render_resources::{MeshData, TerrainMesh},
            world::terrain::mesh_key,
        },
        rendering_context::RenderingContext,
        terrain_generation::{
            ChunkStreamer, LodFocus, LodGrid, MeshCache, TerrainSnapshot, mesh_chunk,
        },
    },
    profile,
};

/// Lot de re-maillage **en vol** : ce que les threads de fond sont en train de produire,
/// plus ce que le cache a déjà fourni.
///
/// ## Pourquoi un lot entier, et pas chunk par chunk
/// Les chunks ne finissent pas ensemble. Installer A pendant que son voisin B porte
/// encore l'ancien masque de coutures ouvre une fissure pendant quelques frames. On
/// accumule donc tout et on ne bascule que **le lot complet** ; entre-temps le terrain
/// reste cohérent avec l'ancienne configuration. Afficher un LOD périmé ~20 frames est
/// invisible ; une fissure ne l'est pas.
///
/// Exception : le **chargement initial** ([`MeshJob::initial_load`]) est progressif.
/// Tous ses chunks sont maillés contre la même grille et aucun ancien mesh n'existe, donc
/// deux chunks installés sont toujours cohérents entre eux.
///
/// ## Trois phases, dont deux étalées dans le temps
/// ```text
/// 1. mailler       → threads rayon, hors frame                (2–10 ms/chunk)
/// 2. construire    → buffers Vulkan, étalé sur N frames       (~1 ms/chunk !)
/// 3. installer     → échange de HashMap, une seule frame      (~µs)
/// ```
/// La phase 2 est le piège : `TerrainMesh::new` fait **4 allocations Vulkan** par chunk
/// (create + allocate, pour le vertex et l'index buffer) plus le memcpy vers le staging.
/// `vkAllocateMemory` est une opération noyau à ~100 µs–1 ms — bâtir 200 meshes d'un coup
/// coûte donc des centaines de millisecondes, soit une image figée à chaque lot.
/// On construit sous **budget de temps** (`frame_budget_ms` de `render.ron`) sans rien installer :
/// l'atomicité reste intacte, seule la latence du lot augmente de quelques frames.
///
/// La phase 3 est la seule à toucher l'état du terrain : elle est donc restée sur
/// [`Terrain`](super::Terrain), qui consomme le lot via [`MeshJob::take_built`].
pub struct MeshJob {
    /// Résultats des workers rayon. Se déconnecte quand tous ont fini.
    receiver: Receiver<Meshed>,
    /// Géométries déjà rassemblées : hits du cache dès la soumission, puis résultats
    /// des workers au fil des frames.
    gathered: Vec<(IVec2, Arc<MeshData>)>,
    /// Tous les workers ont rendu leur `sender` : `gathered` ne grandira plus.
    all_meshed: bool,
    /// Index du prochain élément de `gathered` dont il faut bâtir les buffers.
    next_build: usize,
    /// Zone d'attente : buffers déjà bâtis, pas encore visibles. `None` = chunk devenu
    /// vide ou sorti de la fenêtre (il faudra retirer son mesh sans en installer d'autre).
    built: Vec<(IVec2, Option<TerrainMesh>)>,
    /// Configuration LOD contre laquelle ce lot a été maillé.
    grid: Arc<LodGrid>,
    /// Chunks confiés aux workers ; le reste du lot venait du cache.
    meshed_count: usize,
    /// Installer au fil de l'eau plutôt qu'en bloc (chargement initial uniquement).
    progressive: bool,
    /// Temps de construction de buffers autorisé par frame.
    budget: Duration,
    /// Soumission du lot, pour mesurer sa durée totale.
    started: Instant,
    /// Durée du maillage (soumission → dernier worker fini), une fois connue.
    meshing_time: Option<Duration>,
    /// Par LOD : chunks maillés et temps CPU cumulé de `mesh_chunk`. Agrandi à la
    /// demande : le nombre de niveaux vient de la config.
    mesh_cost: Vec<(u32, Duration)>,
}

/// Résultat d'un worker : le chunk, sa géométrie, le temps passé à la mailler.
type Meshed = (IVec2, MeshData, Duration);

impl MeshJob {
    /// Demande une nouvelle configuration (fenêtre + LOD) au `streamer` et, si elle
    /// change quelque chose, soumet le lot correspondant. `None` = rien à faire.
    pub(super) fn start(
        streamer: &mut ChunkStreamer,
        cache: &mut MeshCache,
        snapshot: TerrainSnapshot,
        focus: LodFocus,
        budget: Duration,
    ) -> Option<MeshJob> {
        let update = streamer.update(focus)?;
        let mut job = Self::submit(update.grid, update.dirty, cache, snapshot, false, budget);
        // Les sortants partent avec le lot, pas avant : retirer un chunk change les
        // coutures de ses voisins, re-maillés dans ce même lot.
        job.built
            .extend(update.removed.into_iter().map(|coord| (coord, None)));
        Some(job)
    }

    /// Lot du chargement initial : tous les chunks de `grid`, installés **au fil de
    /// l'eau** dans l'ordre de `coords` (trié du plus proche au plus lointain par
    /// l'appelant), sous un budget par frame plus large que celui du LOD.
    pub(super) fn start_initial_load(
        grid: Arc<LodGrid>,
        coords: Vec<IVec2>,
        cache: &mut MeshCache,
        snapshot: TerrainSnapshot,
        budget: Duration,
    ) -> MeshJob {
        Self::submit(grid, coords, cache, snapshot, true, budget)
    }

    /// Répartit `dirty` entre cache et workers, et lance ces derniers.
    ///
    /// Le partage se fait ici, à la soumission : un hit économise 2–10 ms de maillage
    /// contre ~50 µs de ré-upload.
    fn submit(
        grid: Arc<LodGrid>,
        dirty: Vec<IVec2>,
        cache: &mut MeshCache,
        snapshot: TerrainSnapshot,
        progressive: bool,
        budget: Duration,
    ) -> MeshJob {
        let started = Instant::now();
        let mut gathered = Vec::with_capacity(dirty.len());
        let mut todo = Vec::new();
        for coord in dirty {
            match cache.get(mesh_key(&grid, coord)) {
                Some(data) => gathered.push((coord, data)),
                None => todo.push(coord),
            }
        }

        let meshed_count = todo.len();
        let (sender, receiver) = channel();
        if todo.is_empty() {
            // Lot entièrement servi par le cache : le canal se ferme aussitôt, la
            // collecte qui suit commitera dans cette même frame.
            drop(sender);
        } else {
            dispatch(todo, snapshot, grid.clone(), sender);
        }

        MeshJob {
            receiver,
            gathered,
            all_meshed: false,
            next_build: 0,
            built: Vec::new(),
            grid,
            meshed_count,
            progressive,
            budget,
            started,
            meshing_time: None,
            mesh_cost: Vec::new(),
        }
    }

    /// Phase 1 : ramasse ce que les workers ont produit depuis la dernière frame.
    /// Ne fait qu'accumuler; rien n'est visible avant l'installation.
    pub(super) fn collect(&mut self) {
        loop {
            match self.receiver.try_recv() {
                Ok((coord, data, time)) => {
                    let lod = self.grid.lod(coord) as usize;
                    if lod >= self.mesh_cost.len() {
                        self.mesh_cost.resize(lod + 1, (0, Duration::ZERO));
                    }
                    let cost = &mut self.mesh_cost[lod];
                    cost.0 += 1;
                    cost.1 += time;
                    self.gathered.push((coord, Arc::new(data)));
                }
                // Rien de neuf pour l'instant ; on repassera à la frame suivante.
                Err(TryRecvError::Empty) => return,
                // Tous les workers ont rendu leur `sender` ⇒ plus rien n'arrivera.
                Err(TryRecvError::Disconnected) => {
                    if !self.all_meshed {
                        self.meshing_time = Some(self.started.elapsed());
                    }
                    self.all_meshed = true;
                    return;
                }
            }
        }
    }

    /// Phase 2 : bâtit les buffers Vulkan des géométries reçues, **sous budget de temps**
    /// et sans rien rendre visible. `true` ⇒ le lot est complet.
    ///
    /// C'est ici qu'était le pic : 4 allocations Vulkan par chunk × 200 chunks dans une
    /// seule frame donnaient une image de plusieurs centaines de millisecondes.
    pub(super) fn build(
        &mut self,
        cache: &mut MeshCache,
        context: &Arc<RenderingContext>,
    ) -> Result<bool> {
        profile!();
        let deadline = Instant::now() + self.budget;

        while self.next_build < self.gathered.len() {
            let (coord, data) = self.gathered[self.next_build].clone();
            self.next_build += 1;

            // Le cache peut être alimenté dès maintenant : il n'a aucun effet sur ce qui
            // est affiché, seulement sur ce qu'on saura ne pas recalculer plus tard.
            cache.insert(mesh_key(&self.grid, coord), data.clone());

            // Chunk devenu vide : Vulkan interdit un buffer de taille 0, on note juste
            // qu'il faudra retirer son ancien mesh.
            let mesh = if data.is_empty() {
                None
            } else {
                Some(TerrainMesh::new(context.clone(), data)?)
            };
            self.built.push((coord, mesh));

            // Test après avoir traité un élément : au moins un par frame, toujours.
            if Instant::now() >= deadline {
                return Ok(false);
            }
        }

        // Tout ce qui est arrivé est bâti ; reste à savoir s'il en vient encore.
        Ok(self.all_meshed)
    }

    /// Lot à installer au fil de l'eau plutôt qu'en bloc.
    pub(super) fn is_progressive(&self) -> bool {
        self.progressive
    }

    /// Temps écoulé depuis la soumission du lot.
    pub(super) fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Bilan du maillage : durée murale, puis coût moyen par chunk pour chaque LOD
    /// (temps CPU d'un worker — ce qui permet de comparer les niveaux entre eux).
    pub(super) fn mesh_report(&self) -> String {
        let wall = self.meshing_time.unwrap_or_default().as_millis();
        let per_lod: Vec<String> = self
            .mesh_cost
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| *n > 0)
            .map(|(lod, (n, t))| format!("L{lod} {} µs ×{n}", t.as_micros() / *n as u128))
            .collect();
        format!("maillage {wall} ms [{}]", per_lod.join(", "))
    }

    /// Chunks réellement maillés — le complément vient du cache.
    pub(super) fn meshed_count(&self) -> usize {
        self.meshed_count
    }

    /// Géométries du lot, cache compris. Complètes une fois le lot terminé.
    pub(super) fn gathered(&self) -> &[(IVec2, Arc<MeshData>)] {
        &self.gathered
    }

    /// Configuration LOD du lot.
    pub(super) fn grid(&self) -> &LodGrid {
        &self.grid
    }

    /// Vide la zone d'attente. Un lot atomique ne s'appelle qu'une fois complet ; un lot
    /// progressif, à chaque frame.
    pub(super) fn take_built(&mut self) -> Vec<(IVec2, Option<TerrainMesh>)> {
        std::mem::take(&mut self.built)
    }
}

/// Lance le maillage de `todo` sur tous les threads du pool, **dans l'ordre** de `todo`.
///
/// Pas de `par_iter` : son découpage récursif fait voler aux threads inactifs la plus
/// grosse moitié restante, donc la fin de la liste démarre presque tout de suite. Ici
/// chaque thread prend le prochain index d'un compteur atomique partagé — une file.
/// `spawn_broadcast` rend la main aussitôt : le jeu continue de tourner pendant ce temps.
fn dispatch(
    todo: Vec<IVec2>,
    snapshot: TerrainSnapshot,
    grid: Arc<LodGrid>,
    sender: Sender<Meshed>,
) {
    let next = AtomicUsize::new(0);
    // Tous les threads partagent la closure (donc `sender`) par `&` : `Sender` est `Sync`.
    // Elle meurt après le dernier thread, emportant le `sender`; ce qui déconnecte le canal.
    rayon::spawn_broadcast(move |_| {
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            let Some(&coord) = todo.get(i) else { return };
            let t = Instant::now();
            let data = mesh_chunk(&snapshot, &grid, coord);
            let _ = sender.send((coord, data, t.elapsed()));
        }
    });
}
