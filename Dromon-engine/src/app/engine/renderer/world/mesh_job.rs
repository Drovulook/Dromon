use anyhow::Result;
use ash::vk;
use rayon::prelude::*;
use std::{
    sync::{
        Arc,
        mpsc::{Receiver, channel},
    },
    time::{Duration, Instant},
};

use glam::IVec2;
use std::sync::mpsc::TryRecvError;

use crate::{
    World,
    app::engine::{
        renderer::render_resources::{MeshData, TerrainMesh},
        terrain_generation::{LodFocus, LodGrid, MeshKey, mesh_chunk},
    },
    profile,
};

/// Temps que l'on s'autorise **par frame** à bâtir les buffers Vulkan d'un lot, et autant
/// à détruire les anciens. Un budget en temps plutôt qu'en nombre de meshes : le coût
/// d'une allocation Vulkan varie fortement selon le driver et la taille du chunk, alors
/// qu'une limite en millisecondes s'ajuste d'elle-même. Au moins un mesh est toujours
/// traité, pour garantir la progression même si le budget est déjà dépassé.
///
/// 2 ms sur les 16 d'une frame à 60 fps : le lot met quelques frames de plus à
/// apparaître, ce qui ne se voit pas.
const BUILD_BUDGET: Duration = Duration::from_millis(2);

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
/// On construit sous **budget de temps** (cf. [`BUILD_BUDGET`]) sans rien installer :
/// l'atomicité reste intacte, seule la latence du lot augmente de quelques frames.
pub struct MeshJob {
    /// Résultats des workers rayon. Se déconnecte quand tous ont fini.
    receiver: Receiver<(IVec2, MeshData)>,
    /// Géométries déjà rassemblées : hits du cache dès la soumission, puis résultats
    /// des workers au fil des frames.
    gathered: Vec<(IVec2, Arc<MeshData>)>,
    /// Tous les workers ont rendu leur `sender` : `gathered` ne grandira plus.
    all_meshed: bool,
    /// Index du prochain élément de `gathered` dont il faut bâtir les buffers.
    next_build: usize,
    /// Zone d'attente : buffers déjà bâtis, pas encore visibles. `None` = chunk devenu
    /// vide (il faudra retirer son mesh sans en installer d'autre).
    built: Vec<(IVec2, Option<TerrainMesh>)>,
    /// Configuration LOD contre laquelle ce lot a été maillé.
    grid: Arc<LodGrid>,
    /// Chunks confiés aux workers (le reste venait du cache) — pour le log.
    meshed_count: usize,
}

impl World {
    // ─── Terrain vivant ──────────────────────────────────────────────────────

    /// **Point d'accroche unique du terrain qui suit la caméra**, appelé une fois par
    /// frame. Aujourd'hui : recalcul du niveau de détail. Demain, au même endroit :
    /// chargement des chunks qui entrent dans la portée et déchargement de ceux qui en
    /// sortent — le pipeline (calculer une configuration cible → mailler en fond →
    /// basculer le lot d'un coup) est déjà celui qu'il faudra.
    ///
    /// Ne fait presque rien la plupart du temps : le premier filtre est une comparaison
    /// de distances au carré (cf. `LodUpdater::update`).
    pub fn update_terrain(&mut self) -> Result<()> {
        profile!();
        self.frame += 1;
        self.bury_expired_meshes();
        self.start_mesh_job();
        self.collect_mesh_job();
        self.build_mesh_job()
    }

    /// Enregistre les copies staging → device des meshes fraîchement installés.
    /// À appeler en tête de frame, **hors** de tout rendering scope.
    pub fn record_terrain_uploads(&mut self, command_buffer: vk::CommandBuffer) {
        if self.pending_uploads.is_empty() {
            return;
        }
        profile!();
        for coord in std::mem::take(&mut self.pending_uploads) {
            if let Some(mesh) = self.terrain_meshes.get(&coord) {
                mesh.record_upload(&command_buffer);
            }
        }

        // Une seule barrière pour tout le lot : les copies doivent être visibles de
        // l'étage d'assemblage des sommets avant le premier draw de la frame.
        unsafe {
            self.context.device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::VERTEX_INPUT,
                vk::DependencyFlags::empty(),
                &[vk::MemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(
                        vk::AccessFlags::VERTEX_ATTRIBUTE_READ | vk::AccessFlags::INDEX_READ,
                    )],
                &[],
                &[],
            );
        }
    }

    /// Clé de cache d'un chunk sous une configuration donnée.
    pub fn mesh_key(grid: &LodGrid, coord: IVec2) -> MeshKey {
        MeshKey {
            coord,
            lod: grid.lod(coord),
            faces: grid.faces(coord),
        }
    }

    /// Réévalue le LOD et, si des chunks changent de géométrie, lance un lot.
    ///
    /// **Un seul lot à la fois** : deux lots concurrents mélangeraient deux
    /// configurations LOD au moment du commit, donc rouvriraient des fissures. Tant
    /// qu'un lot est en vol on ne touche même pas à l'updater, qui garde son point focal
    /// de référence — le prochain appel repartira de là.
    fn start_mesh_job(&mut self) {
        if self.mesh_job.is_some() {
            return;
        }
        let (Some(manager), Some(updater)) =
            (self.chunk_manager.clone(), self.lod_updater.as_mut())
        else {
            return;
        };

        let focus = LodFocus::new(self.camera.position, self.terrain_reference_z);
        let Some(update) = updater.update(focus) else {
            return;
        };
        let grid = update.grid;

        // Partage entre ce qu'on connaît déjà et ce qu'il faut vraiment mailler. Un hit
        // économise 2–10 ms de maillage contre ~50 µs de ré-upload.
        let mut gathered = Vec::with_capacity(update.dirty.len());
        let mut todo = Vec::new();
        for &coord in &update.dirty {
            let key = Self::mesh_key(&grid, coord);
            match self.mesh_cache.get(key) {
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
            let grid_for_job = grid.clone();
            // `rayon::spawn` rend la main tout de suite : le `par_iter` s'exécute sur le
            // pool pendant que le jeu continue d'afficher les anciens meshes.
            rayon::spawn(move || {
                todo.into_par_iter().for_each_with(sender, |sender, coord| {
                    let data = mesh_chunk(&manager, &grid_for_job, coord);
                    let _ = sender.send((coord, data));
                });
            });
        }

        self.mesh_job = Some(MeshJob {
            receiver,
            gathered,
            all_meshed: false,
            next_build: 0,
            built: Vec::new(),
            grid,
            meshed_count,
        });
    }

    /// Phase 1 : ramasse ce que les workers ont produit depuis la dernière frame.
    /// Ne fait qu'accumuler — rien n'est visible avant l'installation.
    fn collect_mesh_job(&mut self) {
        let Some(job) = self.mesh_job.as_mut() else {
            return;
        };
        loop {
            match job.receiver.try_recv() {
                Ok((coord, data)) => job.gathered.push((coord, Arc::new(data))),
                // Rien de neuf pour l'instant ; on repassera à la frame suivante.
                Err(TryRecvError::Empty) => return,
                // Tous les workers ont rendu leur `sender` ⇒ plus rien n'arrivera.
                Err(TryRecvError::Disconnected) => {
                    job.all_meshed = true;
                    return;
                }
            }
        }
    }

    /// Phase 2 : bâtit les buffers Vulkan des géométries reçues, **sous budget de temps**
    /// et sans rien rendre visible. Quand tout le lot est bâti, enchaîne sur la phase 3.
    ///
    /// C'est ici qu'était le pic : 4 allocations Vulkan par chunk × 200 chunks dans une
    /// seule frame donnaient une image de plusieurs centaines de millisecondes.
    fn build_mesh_job(&mut self) -> Result<()> {
        if self.mesh_job.is_none() {
            return Ok(());
        }
        profile!();
        let deadline = Instant::now() + BUILD_BUDGET;
        let context = self.context.clone();

        loop {
            let job = self.mesh_job.as_mut().expect("testé juste au-dessus");
            if job.next_build >= job.gathered.len() {
                break;
            }
            let (coord, data) = job.gathered[job.next_build].clone();
            job.next_build += 1;
            let key = Self::mesh_key(&job.grid, coord);

            // Le cache peut être alimenté dès maintenant : il n'a aucun effet sur ce qui
            // est affiché, seulement sur ce qu'on saura ne pas recalculer plus tard.
            self.mesh_cache.insert(key, data.clone());

            // Chunk devenu vide : Vulkan interdit un buffer de taille 0, on note juste
            // qu'il faudra retirer son ancien mesh.
            let mesh = if data.is_empty() {
                None
            } else {
                Some(TerrainMesh::new(context.clone(), data)?)
            };
            self.mesh_job
                .as_mut()
                .expect("toujours présent")
                .built
                .push((coord, mesh));

            // Test après avoir traité un élément : au moins un par frame, toujours.
            if Instant::now() >= deadline {
                return Ok(());
            }
        }

        let job = self.mesh_job.as_ref().expect("testé en tête");
        if !job.all_meshed {
            return Ok(()); // tout ce qui est arrivé est bâti, mais il en reste à venir
        }
        let job = self.mesh_job.take().expect("testé en tête");
        self.install_mesh_job(job);
        Ok(())
    }

    /// Phase 3 : bascule le lot entier. Ne fait que déplacer des `TerrainMesh` déjà
    /// construits — quelques microsecondes, donc sans risque pour la frame.
    fn install_mesh_job(&mut self, job: MeshJob) {
        profile!();
        let batch = job.built.len();
        for (coord, mesh) in job.built {
            if let Some(old) = self.terrain_meshes.remove(&coord) {
                self.graveyard.push((old, self.frame));
            }
            if let Some(mesh) = mesh {
                self.terrain_meshes.insert(coord, mesh);
                self.pending_uploads.push(coord);
            }
        }

        // Trace de contrôle : valide les estimations de charge (nombre de chunks sales
        // par franchissement de seuil) et donne le taux de réutilisation du cache, seule
        // façon de savoir s'il est rentable pour un style de déplacement donné.
        let (hits, misses) = self.mesh_cache.stats();
        let (bytes, entries) = self.mesh_cache.usage();
        self.logger.info(&format!(
            "LOD : {batch} chunks installés ({} maillés, {} repris du cache) — cache {hits}/{} accès, {entries} entrées, {} Mo",
            job.meshed_count,
            batch.saturating_sub(job.meshed_count),
            hits + misses,
            bytes / (1024 * 1024),
        ));
    }

    /// Détruit les meshes dont plus aucune frame en vol ne peut se servir, **sous le même
    /// budget de temps** que la construction : `Buffer::drop` appelle `vkDestroyBuffer` +
    /// `vkFreeMemory`, soit 4 opérations noyau par mesh — libérer 200 anciens meshes d'un
    /// coup coûterait exactement le pic qu'on vient d'éliminer à la construction.
    /// Garder un mesh mort quelques frames de plus ne coûte que de la mémoire.
    ///
    /// Un mesh est retiré à la frame `f` **avant** l'enregistrement du command buffer de
    /// `f` : seules les frames `< f` ont pu le référencer. Attendre `frames_in_flight + 1`
    /// frames laisse à toutes le temps d'être signalées, avec une frame de marge (le
    /// rebut a lieu avant le `wait_for_fences` de la frame courante).
    fn bury_expired_meshes(&mut self) {
        if self.graveyard.is_empty() {
            return;
        }
        profile!();
        let deadline = Instant::now() + BUILD_BUDGET;
        let (frame, guard) = (self.frame, self.frames_in_flight + 1);

        let mut i = 0;
        while i < self.graveyard.len() {
            if frame < self.graveyard[i].1 + guard {
                i += 1;
                continue;
            }
            // `swap_remove` rend l'élément, qui est droppé ici : c'est la destruction
            // Vulkan effective. L'ordre du cimetière n'a aucune importance.
            self.graveyard.swap_remove(i);
            if Instant::now() >= deadline {
                return;
            }
        }
    }
}
