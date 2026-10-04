pub(crate) mod culling;
mod generate;
mod graveyard;
mod mesh_job;
pub(crate) mod meshes;

use anyhow::Result;
use ash::vk;
use glam::IVec2;
use std::sync::Arc;
use std::time::Duration;

use crate::app::engine::renderer::camera::Camera;
use crate::app::engine::renderer::render_resources::TerrainMesh;
use crate::app::engine::renderer::world::light::DirectionalLight;
use crate::app::engine::renderer::world::terrain::culling::VisibleSet;
use crate::app::engine::renderer::world::terrain::generate::log_terrain_stats;
use crate::app::engine::renderer::world::terrain::graveyard::Graveyard;
use crate::app::engine::renderer::world::terrain::mesh_job::MeshJob;
use crate::app::engine::renderer::world::terrain::meshes::InstalledChunks;
use crate::app::engine::rendering_context::RenderingContext;
use crate::app::engine::terrain_generation::{
    ChunkStore, ChunkStreamer, LodFocus, LodGrid, MeshCache, MeshKey, TerrainSnapshot,
    TerrainSource,
};
use crate::app::logger::Logger;
use crate::config::DebugConfig;
use crate::profile;

/// **Le terrain vivant** : le relief, ce que le joueur en a modifié, les meshes GPU
/// installés, et toute la machinerie qui fait suivre les seconds aux mouvements de caméra.
///
/// Existe (`World::terrain` est `Some`) dès que la scène a appelé
/// [`World::generate_terrain`](super::World::generate_terrain), et pas avant. Regrouper
/// ces champs rend l'invariant structurel : il n'y avait aucun moyen d'avoir un
/// streamer sans relief quand les deux étaient deux `Option` séparées sur `World`.
pub struct Terrain {
    /// Le relief procédural. Immuable, partagé en `Arc` avec les threads de maillage.
    source: Arc<TerrainSource>,
    /// Les édits du joueur : la seule donnée du monde qui ne se recalcule pas. Chaque lot
    /// de maillage en emporte un instantané figé plutôt que la version vivante.
    pub store: ChunkStore,
    /// Un `InstalledChunk` par chunk **non vide**, indexé par coordonnée : c'est ce qui
    /// permet de remplacer le mesh d'un chunk précis quand son LOD change (un `Vec`
    /// n'avait aucun lien avec les positions, les chunks vides décalant même les index).
    pub chunks: InstalledChunks,
    /// Chunks chargés et leur LOD, suivant la caméra.
    streamer: ChunkStreamer,
    /// Altitude moyenne du relief : plan de référence de la composante « hauteur de
    /// caméra » du LOD (cf. [`LodFocus`]).
    reference_z: f32,
    /// Géométries déjà calculées, gardées pour les allers-retours de caméra.
    mesh_cache: MeshCache,
    /// Lot de re-maillage en cours, s'il y en a un.
    mesh_job: Option<MeshJob>,
    /// Chunks dont les buffers attendent leur copie staging → device.
    pending_uploads: Vec<IVec2>,
    /// Meshes remplacés, détruits une fois qu'aucune frame en vol ne peut plus les lire.
    graveyard: Graveyard,
    /// Chunks retenus par le frustum culling, recalculés à chaque frame.
    pub visible: VisibleSet,
    /// Temps de construction de buffers autorisé par frame (`render.ron`). 1 ms sur les
    /// 16 d'une frame à 60 fps : un lot met quelques frames de plus à apparaître, ce qui
    /// ne se voit pas.
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
    /// cible → mailler en fond → basculer le lot d'un coup).
    ///
    /// Ne fait presque rien la plupart du temps : le premier filtre est une comparaison
    /// de distances au carré (cf. `ChunkStreamer::update`).
    pub fn update(&mut self, camera: &Camera) -> Result<()> {
        profile!();
        self.graveyard.tick();
        self.start_mesh_job(camera);
        self.advance_mesh_job()
    }

    /// Réévalue le LOD et, si des chunks changent de géométrie, lance un lot.
    ///
    /// **Un seul lot à la fois** : deux lots concurrents mélangeraient deux
    /// configurations LOD au moment du commit, donc rouvriraient des fissures. Tant
    /// qu'un lot est en vol on ne touche même pas à l'updater, qui garde son point focal
    /// de référence — le prochain appel repartira de là.
    fn start_mesh_job(&mut self, camera: &Camera) {
        if self.mesh_job.is_some() {
            return;
        }
        // Instantané du terrain pour ce lot : relief immuable + `Arc` des chunks édités
        // tels qu'ils sont maintenant. Le joueur peut creuser pendant le maillage sans
        // que les workers voient la modification à mi-lot — même garantie que la `grid`.
        let snapshot = TerrainSnapshot::new(&self.source, &self.store, self.debug.mesh);
        let focus = LodFocus::new(camera.position, self.reference_z);
        self.mesh_job = MeshJob::start(
            &mut self.streamer,
            &mut self.mesh_cache,
            snapshot,
            focus,
            self.frame_budget,
        );
    }

    /// Fait avancer le lot en vol d'une frame, et l'installe dès qu'il est complet, ou
    /// au fil de l'eau s'il est progressif.
    ///
    /// Les trois emprunts (`mesh_job`, `mesh_cache`, `context`) portent sur des **champs
    /// distincts** de `self` : le borrow checker les accepte simultanément, ce qu'il
    /// refuserait à travers des méthodes prenant `&mut self`. C'est toute la raison pour
    /// laquelle les phases vivent sur `MeshJob` et non sur `Terrain`.
    fn advance_mesh_job(&mut self) -> Result<()> {
        let Some(job) = self.mesh_job.as_mut() else {
            return Ok(());
        };
        job.collect();
        let complete = job.build(&mut self.mesh_cache, &self.context)?;
        if job.is_progressive() {
            let built = job.take_built();
            self.install(built);
        }
        if !complete {
            return Ok(());
        }
        let mut job = self
            .mesh_job
            .take()
            .expect("présent : `build` vient de signaler le lot complet");
        self.install(job.take_built());
        if job.is_progressive() {
            self.logger.info(&format!(
                "Génération du monde terminée : {} chunks en {:.2} s",
                job.gathered().len(),
                job.elapsed().as_secs_f32(),
            ));
            log_terrain_stats(&self.logger, job.grid(), job.gathered(), self.max_lod);
        }
        self.log_batch_stats(job.gathered().len(), job.meshed_count(), &job.mesh_report());
        self.log_gpu_usage();
        Ok(())
    }

    /// Bascule des meshes bâtis. Ne fait que déplacer des `TerrainMesh` déjà construits;
    /// quelques microsecondes, donc sans risque pour la frame.
    fn install(&mut self, built: Vec<(IVec2, Option<TerrainMesh>)>) {
        if built.is_empty() {
            return;
        }
        profile!();
        for (coord, mesh) in built {
            match mesh {
                // Remplacement : `insert` rend l'ancien, la coordonnée ne bouge pas.
                Some(mesh) => {
                    if let Some(old) = self.chunks.insert(coord, mesh) {
                        self.graveyard.bury(old);
                    }
                    self.pending_uploads.push(coord);
                }
                // Le chunk devient vide : vraie suppression.
                None => {
                    if let Some(old) = self.chunks.remove(&coord) {
                        self.graveyard.bury(old);
                    }
                }
            }
        }
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

    /// Recalcule les chunks visibles depuis la caméra et depuis la lumière.
    pub fn update_visibility(&mut self, camera: &Camera, light: &DirectionalLight) {
        let max_height = self.source.max_height() as f32;
        self.visible.update(&self.chunks, camera, light, max_height);
    }

    /// Trace de contrôle d'un lot : valide les estimations de charge (combien de chunks
    /// deviennent sales à chaque franchissement de seuil) et donne le taux de réutilisation
    /// du cache, seule façon de savoir s'il est rentable pour un style de déplacement
    /// donné, la réponse allant de ~0 % en exploration rectiligne à très rentable pour un
    /// joueur qui tourne autour d'une base.
    ///
    /// Une ligne par lot peut noyer le reste des logs : `log_batch_stats` de `debug.ron`.
    fn log_batch_stats(&self, batch: usize, meshed: usize, mesh_report: &str) {
        if !self.debug.log_batch_stats {
            return;
        }
        let (hits, misses) = self.mesh_cache.stats();
        let (bytes, entries) = self.mesh_cache.usage();
        self.logger.info(&format!(
            "LOD : {batch} chunks installés ({meshed} maillés, {} repris du cache), {mesh_report} — cache {hits}/{} accès, {entries} entrées, {} Mo",
            batch.saturating_sub(meshed),
            hits + misses,
            bytes / (1024 * 1024),
        ));
    }

    /// Surveillance de la fragmentation externe de l'allocateur GPU, à l'installation
    /// d'un lot : il vient de libérer les anciens meshes et d'en allouer autant de
    /// nouveaux, c'est le moment où les trous apparaissent.
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

/// Clé de cache d'un chunk sous une configuration LOD donnée.
fn mesh_key(grid: &LodGrid, coord: IVec2) -> MeshKey {
    MeshKey {
        coord,
        lod: grid.lod(coord),
        faces: grid.faces(coord),
    }
}
