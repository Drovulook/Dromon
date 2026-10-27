use anyhow::Result;
use glam::IVec2;
use rustc_hash::FxHashMap;
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
            world::terrain::{mesh_key, meshes::InstalledChunks},
        },
        rendering_context::RenderingContext,
        terrain_generation::{LodGrid, MeshCache, TerrainSnapshot, mesh_chunk},
    },
    profile,
};

/// Lot de re-maillage **en vol** : ce que les threads de fond sont en train de produire,
/// plus ce que le cache a déjà fourni.
///
/// ## Bascule par groupes, pas chunk par chunk
/// Les chunks ne finissent pas ensemble. Installer le nouveau A à côté de l'ancien B
/// ouvre une fissure si B change lui aussi : chacun a été maillé contre une autre
/// configuration de leur frontière. Le lot est donc découpé en **groupes** (cf.
/// [`groups`]) qui basculent chacun d'un bloc, dès que tous leurs membres sont bâtis.
///
/// Pourquoi c'est sûr : toute paire de voisins affichés est maillée contre une même
/// grille.
/// - deux membres d'un groupe : la nouvelle ;
/// - un chunk **hors lot** et n'importe qui : son mesh est identique sous l'ancienne et
///   la nouvelle grille (cf. `LodGrid::dirty_against`) ;
/// - deux groupes distincts ne se touchent pas, par construction.
///
/// Le **chargement initial** n'a aucun ancien mesh : chaque chunk y forme son propre
/// groupe, d'où une apparition progressive sans traitement spécial.
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
/// On construit sous **budget de temps** (`frame_budget_ms` de `render.ron`) sans rien
/// installer : l'atomicité des groupes reste intacte, seule leur latence augmente.
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
    /// Groupes complets, prêts à installer. `None` = chunk devenu vide (il faudra retirer
    /// son mesh sans en installer d'autre).
    built: Vec<(IVec2, Option<TerrainMesh>)>,
    /// Groupe de bascule de chaque chunk du lot.
    group_of: FxHashMap<IVec2, u32>,
    /// Par groupe : membres pas encore bâtis.
    remaining: Vec<u32>,
    /// Prochain groupe à installer : les groupes basculent dans l'ordre de leur numéro.
    next_install: usize,
    /// Par groupe : buffers bâtis, invisibles tant que le groupe n'est pas complet.
    staged: Vec<Vec<(IVec2, Option<TerrainMesh>)>>,
    /// Taille du plus gros groupe, pour les statistiques.
    largest_group: u32,
    /// Configuration LOD contre laquelle ce lot a été maillé.
    grid: Arc<LodGrid>,
    /// Chunks confiés aux workers ; le reste du lot venait du cache.
    meshed_count: usize,
    /// Lot du chargement initial (seule différence : le bilan loggé à la fin).
    initial: bool,
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
    /// Lot de re-maillage des chunks `dirty` (triés du plus proche au plus lointain par
    /// l'appelant) contre la configuration `grid`. `installed` dit quels chunks ont un
    /// ancien mesh affiché, ce qui décide des groupes de bascule.
    ///
    /// Les workers traitent le lot **groupe par groupe** : d'abord ceux qui contiennent un
    /// ancien mesh, puis les groupes d'entrants seuls ; chaque catégorie du plus proche au
    /// plus lointain. Un groupe à ancien mesh peut contenir des entrants (la rangée qui
    /// borde l'ancien terrain) et attend son membre le plus lent : un entrant isolé
    /// affiché avant lui, de l'autre côté de cette rangée, l'enclaverait — une déchirure.
    /// Cas typique : la caméra a dépassé la fenêtre, ses entrants sont les plus proches.
    pub(super) fn start(
        grid: Arc<LodGrid>,
        mut dirty: Vec<IVec2>,
        installed: &InstalledChunks,
        cache: &mut MeshCache,
        snapshot: TerrainSnapshot,
        budget: Duration,
    ) -> MeshJob {
        let has_old = |c: IVec2| installed.get(&c).is_some();
        let (mut group_of, sizes) = groups(&dirty, has_old);
        let mut group_has_old = vec![false; sizes.len()];
        for &c in &dirty {
            group_has_old[group_of[&c] as usize] |= has_old(c);
        }
        // Numéro de groupe = rang de son membre le plus proche (cf. `groups`). Tri stable :
        // l'ordre par distance tient à l'intérieur de chaque groupe.
        dirty.sort_by_key(|c| {
            let g = group_of[c];
            (!group_has_old[g as usize], g)
        });

        // Renumérotation dans l'ordre de traitement : `build` installe les groupes dans
        // l'ordre de leurs numéros.
        let mut new_id = vec![u32::MAX; sizes.len()];
        let mut new_sizes = Vec::with_capacity(sizes.len());
        for c in &dirty {
            let g = group_of[c] as usize;
            if new_id[g] == u32::MAX {
                new_id[g] = new_sizes.len() as u32;
                new_sizes.push(sizes[g]);
            }
        }
        for g in group_of.values_mut() {
            *g = new_id[*g as usize];
        }
        let sizes = new_sizes;

        Self::submit(grid, dirty, group_of, sizes, cache, snapshot, false, budget)
    }

    /// Lot du chargement initial : tous les chunks de `grid`, dans l'ordre de `coords`
    /// (trié du plus proche au plus lointain par l'appelant). Aucun ancien mesh, donc un
    /// groupe par chunk : chacun s'affiche dès qu'il est bâti.
    pub(super) fn start_initial_load(
        grid: Arc<LodGrid>,
        coords: Vec<IVec2>,
        cache: &mut MeshCache,
        snapshot: TerrainSnapshot,
        budget: Duration,
    ) -> MeshJob {
        let (group_of, sizes) = groups(&coords, |_| false);
        Self::submit(grid, coords, group_of, sizes, cache, snapshot, true, budget)
    }

    /// Répartit `dirty` entre cache et workers, et lance ces derniers.
    ///
    /// Le partage se fait ici, à la soumission : un hit économise 2–10 ms de maillage
    /// contre ~50 µs de ré-upload.
    #[allow(clippy::too_many_arguments)]
    fn submit(
        grid: Arc<LodGrid>,
        dirty: Vec<IVec2>,
        group_of: FxHashMap<IVec2, u32>,
        sizes: Vec<u32>,
        cache: &mut MeshCache,
        snapshot: TerrainSnapshot,
        initial: bool,
        budget: Duration,
    ) -> MeshJob {
        let started = Instant::now();
        let largest_group = sizes.iter().copied().max().unwrap_or(0);
        let staged = (0..sizes.len()).map(|_| Vec::new()).collect();
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
            group_of,
            remaining: sizes,
            next_install: 0,
            staged,
            largest_group,
            grid,
            meshed_count,
            initial,
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

    /// Phase 2 : bâtit les buffers Vulkan des géométries reçues, **sous budget de temps**,
    /// et passe chaque groupe complet dans la zone `built`. `true` ⇒ le lot est complet.
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
            let g = self.group_of[&coord] as usize;
            self.staged[g].push((coord, mesh));
            self.remaining[g] -= 1;
            // Installation strictement dans l'ordre des groupes : ni un hit de cache (bâti
            // dès la 1re frame) ni un worker rapide ne doit doubler un groupe précédent, sinon
            // il s'affiche au-delà d'un trou (îlot lointain, rangée manquante enclavée).
            while self.next_install < self.remaining.len() && self.remaining[self.next_install] == 0
            {
                self.built.append(&mut self.staged[self.next_install]);
                self.next_install += 1;
            }

            // Test après avoir traité un élément : au moins un par frame, toujours.
            if Instant::now() >= deadline {
                return Ok(false);
            }
        }

        // Tout ce qui est arrivé est bâti ; reste à savoir s'il en vient encore.
        Ok(self.all_meshed)
    }

    /// Lot du chargement initial ?
    pub(super) fn is_initial(&self) -> bool {
        self.initial
    }

    /// Nombre de groupes de bascule et taille du plus gros.
    pub(super) fn group_stats(&self) -> (usize, u32) {
        (self.remaining.len(), self.largest_group)
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

    /// Vide la zone des groupes complets, à installer. Appelé à chaque frame.
    pub(super) fn take_built(&mut self) -> Vec<(IVec2, Option<TerrainMesh>)> {
        std::mem::take(&mut self.built)
    }
}

/// Groupes de bascule de `dirty` : composantes connexes en **8-voisinage** (un coin est
/// partagé par 4 chunks), deux chunks n'étant liés que si **l'un au moins** a un ancien
/// mesh affiché (`has_old`). Deux entrants voisins restent indépendants : installer l'un
/// sans l'autre laisse un trou, pas une fissure — à condition que ce trou reste en
/// bordure du terrain affiché, ce que garantit l'ordre de traitement (cf. [`MeshJob::start`]).
/// Corollaire : un groupe sans ancien mesh est un entrant seul.
///
/// Renvoie le groupe de chaque chunk et la taille de chaque groupe. Les groupes sont
/// numérotés dans l'ordre de `dirty` : trié par distance, les proches ont les petits
/// numéros.
fn groups(dirty: &[IVec2], has_old: impl Fn(IVec2) -> bool) -> (FxHashMap<IVec2, u32>, Vec<u32>) {
    let index: FxHashMap<IVec2, usize> = dirty.iter().enumerate().map(|(i, &c)| (c, i)).collect();

    // Union-find : `parent[i]` remonte jusqu'au représentant du groupe de `dirty[i]`.
    let mut parent: Vec<usize> = (0..dirty.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]]; // compression de chemin (par sauts de 2)
            i = parent[i];
        }
        i
    }

    // Demi 8-voisinage : chaque paire n'est examinée qu'une fois.
    const HALF: [IVec2; 4] = [
        IVec2::new(1, 0),
        IVec2::new(-1, 1),
        IVec2::new(0, 1),
        IVec2::new(1, 1),
    ];
    let old: Vec<bool> = dirty.iter().map(|&c| has_old(c)).collect();
    for (i, &c) in dirty.iter().enumerate() {
        for off in HALF {
            let Some(&j) = index.get(&(c + off)) else {
                continue;
            };
            if old[i] || old[j] {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                parent[a] = b;
            }
        }
    }

    // Numérotation compacte des représentants, dans l'ordre de `dirty`.
    let mut id_of_root = vec![u32::MAX; dirty.len()];
    let mut sizes = Vec::new();
    let mut group_of = FxHashMap::default();
    for (i, &c) in dirty.iter().enumerate() {
        let root = find(&mut parent, i);
        if id_of_root[root] == u32::MAX {
            id_of_root[root] = sizes.len() as u32;
            sizes.push(0);
        }
        let g = id_of_root[root];
        sizes[g as usize] += 1;
        group_of.insert(c, g);
    }
    (group_of, sizes)
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
            // Récepteur détruit (terrain régénéré entre-temps) : le reste du lot ne
            // servirait plus à rien.
            if sender.send((coord, data, t.elapsed())).is_err() {
                return;
            }
        }
    });
}
