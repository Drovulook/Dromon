use anyhow::Result;
use glam::IVec2;
use rayon::prelude::*;
use std::{
    sync::{
        Arc,
        mpsc::{Receiver, TryRecvError, channel},
    },
    time::Instant,
};

use crate::{
    app::engine::{
        renderer::{
            render_resources::{MeshData, TerrainMesh},
            world::terrain::{FRAME_BUDGET, mesh_key},
        },
        rendering_context::RenderingContext,
        terrain_generation::{
            LodFocus, LodGrid, LodUpdater, MeshCache, TerrainSnapshot, mesh_chunk,
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
/// On construit sous **budget de temps** (cf. [`FRAME_BUDGET`]) sans rien installer :
/// l'atomicité reste intacte, seule la latence du lot augmente de quelques frames.
///
/// La phase 3 est la seule à toucher l'état du terrain : elle est donc restée sur
/// [`Terrain`](super::Terrain), qui consomme le lot via [`MeshJob::into_built`].
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
    /// Chunks confiés aux workers ; le reste du lot venait du cache.
    meshed_count: usize,
}

impl MeshJob {
    /// Demande une nouvelle configuration LOD à `updater` et, si elle change quelque
    /// chose, soumet le lot correspondant. `None` = rien à mailler.
    ///
    /// Le partage entre cache et maillage se fait ici, à la soumission : un hit économise
    /// 2–10 ms de maillage contre ~50 µs de ré-upload.
    pub(super) fn start(
        updater: &mut LodUpdater,
        cache: &mut MeshCache,
        snapshot: TerrainSnapshot,
        focus: LodFocus,
    ) -> Option<MeshJob> {
        let update = updater.update(focus)?;
        let grid = update.grid;

        let mut gathered = Vec::with_capacity(update.dirty.len());
        let mut todo = Vec::new();
        for &coord in &update.dirty {
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
            let grid_for_job = grid.clone();
            // `rayon::spawn` rend la main tout de suite : le `par_iter` s'exécute sur le
            // pool pendant que le jeu continue d'afficher les anciens meshes.
            rayon::spawn(move || {
                todo.into_par_iter().for_each_with(sender, |sender, coord| {
                    let data = mesh_chunk(&snapshot, &grid_for_job, coord);
                    let _ = sender.send((coord, data));
                });
            });
        }

        Some(MeshJob {
            receiver,
            gathered,
            all_meshed: false,
            next_build: 0,
            built: Vec::new(),
            grid,
            meshed_count,
        })
    }

    /// Phase 1 : ramasse ce que les workers ont produit depuis la dernière frame.
    /// Ne fait qu'accumuler — rien n'est visible avant l'installation.
    pub(super) fn collect(&mut self) {
        loop {
            match self.receiver.try_recv() {
                Ok((coord, data)) => self.gathered.push((coord, Arc::new(data))),
                // Rien de neuf pour l'instant ; on repassera à la frame suivante.
                Err(TryRecvError::Empty) => return,
                // Tous les workers ont rendu leur `sender` ⇒ plus rien n'arrivera.
                Err(TryRecvError::Disconnected) => {
                    self.all_meshed = true;
                    return;
                }
            }
        }
    }

    /// Phase 2 : bâtit les buffers Vulkan des géométries reçues, **sous budget de temps**
    /// et sans rien rendre visible. `true` ⇒ le lot est complet et prêt à installer.
    ///
    /// C'est ici qu'était le pic : 4 allocations Vulkan par chunk × 200 chunks dans une
    /// seule frame donnaient une image de plusieurs centaines de millisecondes.
    pub(super) fn build(
        &mut self,
        cache: &mut MeshCache,
        context: &Arc<RenderingContext>,
    ) -> Result<bool> {
        profile!();
        let deadline = Instant::now() + FRAME_BUDGET;

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

    /// Chunks réellement maillés — le complément vient du cache.
    pub(super) fn meshed_count(&self) -> usize {
        self.meshed_count
    }

    /// Rend les meshes bâtis à installer. Ne s'appelle qu'après un `build` ayant
    /// renvoyé `true` — le lot est alors définitivement complet.
    pub(super) fn into_built(self) -> Vec<(IVec2, Option<TerrainMesh>)> {
        self.built
    }
}
