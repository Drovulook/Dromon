pub(crate) mod culling;
mod generate;
mod graveyard;
mod loaded_mask;
mod mesh_queue;
pub(crate) mod meshes;

use anyhow::Result;
use ash::vk;
use glam::IVec2;
use rustc_hash::FxHashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::engine::renderer::camera::Camera;
use crate::app::engine::renderer::render_resources::TerrainMesh;
use crate::app::engine::renderer::world::light::DirectionalLight;
use crate::app::engine::renderer::world::terrain::culling::VisibleSet;
use crate::app::engine::renderer::world::terrain::generate::log_terrain_stats;
use crate::app::engine::renderer::world::terrain::graveyard::Graveyard;
use crate::app::engine::renderer::world::terrain::loaded_mask::LoadedMask;
use crate::app::engine::renderer::world::terrain::mesh_queue::MeshQueue;
use crate::app::engine::renderer::world::terrain::meshes::{ChunkSeam, InstalledChunks};
use crate::app::engine::rendering_context::RenderingContext;
use crate::app::engine::terrain_generation::{
    ChunkStore, ChunkStreamer, LodFocus, MeshCache, ShownLods, TerrainSnapshot, TerrainSource,
};
use crate::app::logger::Logger;
use crate::config::{DebugConfig, WorldGenConfig};
use crate::profile;

/// **Le terrain vivant** : le relief, ce que le joueur en a modifié, les meshes GPU
/// installés, et toute la machinerie qui fait suivre les seconds aux mouvements de caméra.
///
/// Existe (`World::terrain` est `Some`) dès que la scène a appelé
/// [`World::generate_terrain`](super::World::generate_terrain), et pas avant. Regrouper
/// ces champs rend l'invariant structurel : il n'y avait aucun moyen d'avoir un
/// streamer sans relief quand les deux étaient deux `Option` séparées sur `World`.
pub struct Terrain {
    /// Début du chargement initial tant qu'il n'est pas terminé.
    loading_since: Option<Instant>,
    /// Dernier bilan de maillage écrit dans les logs.
    last_report: Instant,
    /// Config de génération reçue de la scène : référence du hot reload.
    pub(crate) world_gen: WorldGenConfig,
    /// Le relief procédural. Immuable, partagé en `Arc` avec les threads de maillage.
    source: Arc<TerrainSource>,
    /// Les édits du joueur : la seule donnée du monde qui ne se recalcule pas. Les workers
    /// en reçoivent un instantané figé plutôt que la version vivante.
    pub store: ChunkStore,
    /// Un `InstalledChunk` par chunk **non vide**, indexé par coordonnée : c'est ce qui
    /// permet de remplacer le mesh d'un chunk précis quand son LOD change (un `Vec`
    /// n'avait aucun lien avec les positions, les chunks vides décalant même les index).
    pub chunks: InstalledChunks,
    /// LOD affiché de chaque chunk, vides compris : source des coutures.
    shown: ShownLods,
    /// Les mêmes, en masque pour les shaders de la mer.
    loaded_mask: LoadedMask,
    /// Chunks chargés et leur LOD, suivant la caméra.
    streamer: ChunkStreamer,
    /// Altitude moyenne du relief : plan de référence de la composante « hauteur de
    /// caméra » du LOD (cf. [`LodFocus`]).
    reference_z: f32,
    /// Géométries déjà calculées, gardées pour les allers-retours de caméra.
    mesh_cache: MeshCache,
    /// Rapproche ce qui est affiché de ce qui est voulu.
    mesh_queue: MeshQueue,
    /// Chunks dont les buffers attendent leur copie staging → device.
    pending_uploads: Vec<IVec2>,
    /// Meshes remplacés, détruits une fois qu'aucune frame en vol ne peut plus les lire.
    graveyard: Graveyard,
    /// Chunks retenus par le frustum culling, recalculés à chaque frame.
    pub visible: VisibleSet,
    /// Temps de construction de buffers autorisé par frame (`render.ron`).
    frame_budget: Duration,
    /// LOD maximal (nombre de rayons de `render.ron`), pour les statistiques par niveau.
    max_lod: u8,
    /// Traces et options de maillage (`debug.ron`).
    debug: DebugConfig,
    context: Arc<RenderingContext>,
    logger: Arc<Logger>,
}

impl Terrain {
    /// **Point d'accroche unique du terrain qui suit la caméra**, appelé une fois par
    /// frame : niveau de détail, chargement des chunks qui entrent dans la portée et
    /// retrait de ceux qui en sortent — un seul pipeline (calculer une configuration
    /// cible → mailler en fond → installer chunk par chunk).
    ///
    /// Ne fait presque rien la plupart du temps : le premier filtre est une comparaison
    /// de distances au carré (cf. `ChunkStreamer::update`).
    pub fn update(&mut self, camera: &Camera) -> Result<()> {
        profile!();
        self.graveyard.tick();
        self.retarget(camera);
        self.advance_queue()
    }

    /// Réévalue fenêtre et LOD : retire aussitôt les chunks sortis de la fenêtre et donne
    /// la nouvelle cible à la file.
    fn retarget(&mut self, camera: &Camera) {
        let focus = LodFocus::new(camera.position, self.reference_z);
        let Some(update) = self.streamer.update(focus) else {
            return;
        };
        profile!();
        // Ils sont à la portée de chargement, donc au loin : leurs voisins bordent du vide,
        // un trou à l'horizon, pas une fissure.
        for &coord in &update.removed {
            self.remove(coord);
            self.shown.remove(coord);
        }
        self.refresh_seams(&update.removed);

        // Instantané du terrain : relief immuable + `Arc` des chunks édités tels qu'ils sont
        // maintenant.
        let snapshot =
            TerrainSnapshot::new(&self.source, &self.store, self.debug.mesh, self.max_lod);
        self.mesh_queue.retarget(update.grid, focus, snapshot);
    }

    /// Fait avancer la file d'une frame et installe ce qu'elle a construit.
    ///
    /// Les emprunts (`mesh_queue`, `shown`, `mesh_cache`, `context`) portent sur des
    /// **champs distincts** de `self` : le borrow checker les accepte simultanément, ce
    /// qu'il refuserait à travers des méthodes prenant `&mut self`.
    fn advance_queue(&mut self) -> Result<()> {
        profile!();
        let built = self.mesh_queue.advance(
            &mut self.shown,
            &mut self.mesh_cache,
            &self.context,
        )?;
        self.install(built);
        if self.mesh_queue.is_settled()
            && let Some(start) = self.loading_since.take()
        {
            self.finish_initial_load(start);
        }
        self.log_progress();
        Ok(())
    }

    /// Le monde est affiché : bilan et retour au budget de jeu.
    fn finish_initial_load(&mut self, start: Instant) {
        self.mesh_queue.set_budget(self.frame_budget);
        let meshed: Vec<_> = self
            .chunks
            .coords()
            .iter()
            .filter_map(|c| self.chunks.get(c).map(|chunk| (*c, chunk.mesh.data.clone())))
            .collect();
        self.logger.info(&format!(
            "Génération du monde terminée : {} chunks en {:.2} s",
            meshed.len(),
            start.elapsed().as_secs_f32(),
        ));
        log_terrain_stats(&self.logger, &self.streamer.grid(), &meshed, self.max_lod);
    }

    /// Installe les meshes bâtis et rafraîchit les coutures autour d'eux. Ne fait que
    /// déplacer des `TerrainMesh` déjà construits : sans risque pour la frame.
    fn install(&mut self, built: Vec<(IVec2, Option<TerrainMesh>)>) {
        if built.is_empty() {
            return;
        }
        profile!();
        let coords: Vec<IVec2> = built.iter().map(|&(coord, _)| coord).collect();
        for (coord, mesh) in built {
            match mesh {
                // Remplacement : `insert` rend l'ancien, la coordonnée ne bouge pas.
                Some(mesh) => {
                    let seam = ChunkSeam::new(&self.shown, coord);
                    if let Some(old) = self.chunks.insert(coord, mesh, seam) {
                        self.graveyard.bury(old);
                    }
                    self.loaded_mask.set(coord, true);
                    self.pending_uploads.push(coord);
                }
                // Le chunk devient vide : vraie suppression.
                None => self.remove(coord),
            }
        }
        self.refresh_seams(&coords);
    }

    /// Recalcule la couture des chunks de `changed` et de leurs 8 voisins. Les 4 voisins
    /// directs changent de faces ; les diagonales, d'atténuation (cf. `taper_bits`).
    fn refresh_seams(&mut self, changed: &[IVec2]) {
        if changed.is_empty() {
            return;
        }
        profile!();
        let mut around = FxHashSet::default();
        for &coord in changed {
            for x in -1..=1 {
                for y in -1..=1 {
                    around.insert(coord + IVec2::new(x, y));
                }
            }
        }
        for coord in around {
            self.chunks
                .set_seam(coord, ChunkSeam::new(&self.shown, coord));
        }
    }

    /// Retire le mesh d'un chunk ; ses buffers attendent au cimetière la fin des frames
    /// en vol.
    fn remove(&mut self, coord: IVec2) {
        if let Some(old) = self.chunks.remove(&coord) {
            self.graveyard.bury(old);
        }
        self.loaded_mask.set(coord, false);
    }

    /// Côté du masque des chunks installés (texels), pour créer sa texture.
    pub(crate) fn loaded_mask_size(&self) -> u32 {
        self.loaded_mask.size()
    }

    /// Masque des chunks installés, s'il a changé depuis le dernier appel. À envoyer
    /// dans la frame qui dessine les meshes installés : eau et terrain apparaissent ensemble.
    pub(crate) fn take_loaded_mask(&mut self) -> Option<&[u8]> {
        self.loaded_mask.take_changes()
    }

    /// Enregistre les copies staging → device des meshes fraîchement installés.
    /// À appeler en tête de frame, **hors** de tout rendering scope.
    pub fn record_uploads(&mut self, command_buffer: vk::CommandBuffer) {
        if self.pending_uploads.is_empty() {
            return;
        }
        profile!();
        for coord in std::mem::take(&mut self.pending_uploads) {
            if let Some(chunk) = self.chunks.get(&coord) {
                chunk.mesh.record_upload(&command_buffer);
            }
        }

        // Une seule barrière pour toutes les copies : elles doivent être visibles de
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

    /// Recalcule les chunks visibles depuis la caméra et depuis la lumière.
    pub fn update_visibility(&mut self, camera: &Camera, light: &DirectionalLight) {
        let max_height = self.source.max_height() as f32;
        self.visible.update(&self.chunks, camera, light, max_height);
    }

    /// Hot reload : seules les traces changent à chaud, les options de maillage
    /// (`debug.mesh`) demandent de regénérer le terrain.
    pub(crate) fn hot_reload(&mut self, debug: &DebugConfig) {
        self.debug.log_batch_stats = debug.log_batch_stats;
        self.debug.log_gpu_usage = debug.log_gpu_usage;
    }

    /// Bilan du maillage, au plus une ligne par seconde (`log_batch_stats` de `debug.ron`) :
    /// la charge réelle et le taux de réutilisation du cache, seule façon de savoir s'il
    /// est rentable pour un style de déplacement donné.
    fn log_progress(&mut self) {
        if self.last_report.elapsed() < REPORT_PERIOD {
            return;
        }
        self.last_report = Instant::now();
        let Some(report) = self.mesh_queue.take_report() else {
            return;
        };
        if self.debug.log_batch_stats {
            let (hits, misses) = self.mesh_cache.stats();
            let (bytes, entries) = self.mesh_cache.usage();
            self.logger.info(&format!(
                "LOD : {report} — cache {hits}/{} accès, {entries} entrées, {} Mo",
                hits + misses,
                bytes / (1024 * 1024),
            ));
        }
        self.log_gpu_usage();
    }

    /// Surveillance de la fragmentation externe de l'allocateur GPU.
    fn log_gpu_usage(&self) {
        if !self.debug.log_gpu_usage {
            return;
        }
        const MB: u64 = 1024 * 1024;
        let gpu = self.context.allocator().usage();
        let occupancy = (100 * gpu.used).checked_div(gpu.reserved).unwrap_or(0);
        self.logger.info(&format!(
            "GPU : {} blocs, {}/{} Mo utilisés ({occupancy} %), {} trous, plus grand {} Ko",
            gpu.blocks,
            gpu.used / MB,
            gpu.reserved / MB,
            gpu.holes,
            gpu.largest_free / 1024,
        ));
    }
}

/// Intervalle entre deux bilans de maillage.
const REPORT_PERIOD: Duration = Duration::from_secs(1);
