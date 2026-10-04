use anyhow::Result;
use glam::{IVec2, Vec3};
use std::sync::Arc;
use std::time::Duration;

use crate::{
    World,
    app::{
        engine::{
            renderer::{
                render_resources::MeshData,
                world::{
                    terrain::{
                        Terrain, culling::VisibleSet, graveyard::Graveyard, mesh_job::MeshJob,
                        meshes::InstalledChunks,
                    },
                },
            },
            rendering_context::RenderingContext,
            terrain_generation::{
                ChunkStore, ChunkStreamer, LodFocus, LodGrid, MeshCache, TerrainSnapshot,
                TerrainSource, chunk_distance,
            },
        },
        logger::Logger,
    },
    config::{DebugConfig, TerrainRenderParams, WorldGenConfig},
    profile,
};

impl World {
    /// Crée le terrain de la scène. Appelée par la scène dans `setup`. Rend la main
    /// aussitôt : les chunks sont maillés en fond et apparaissent au fil des frames.
    pub fn generate_terrain(&mut self, config: &WorldGenConfig) -> Result<()> {
        // Départ au-dessus des plus hauts sommets possibles.
        self.camera.position.z = config.world.max_height as f32 * 1.1;

        // Terrain survolé : grande boîte d'ombre qui suit la caméra (`render.ron`).
        self.light.shadow = self.config.render.shadow.terrain.clone();

        self.terrain = Some(Terrain::new(
            config,
            &self.config.render.terrain,
            &self.config.debug,
            self.camera.position,
            self.context.clone(),
            self.logger.clone(),
            self.frames_in_flight,
        ));
        Ok(())
    }
}

impl Terrain {
    /// Crée le monde et lance le maillage en fond de la fenêtre chargée initiale : le
    /// terrain rend la main vide, ses meshes apparaissent au fil des frames suivantes.
    fn new(
        config: &WorldGenConfig,
        params: &TerrainRenderParams,
        debug: &DebugConfig,
        camera_position: Vec3,
        context: Arc<RenderingContext>,
        logger: Arc<Logger>,
        frames_in_flight: u64,
    ) -> Terrain {
        profile!();

        let source = Arc::new(TerrainSource::new(config));

        // Altitude moyenne du relief : plan de référence de la composante verticale du
        //    LOD. Mesurée une seule fois — le relief ne bouge pas.
        let reference_z = source.mean_terrain_height();

        // Fenêtre chargée initiale et ses LOD (distance horizontale ET verticale au
        //    point de vue de départ, équilibrés 2:1 — cf. `LodGrid::rebalance`).
        let focus = LodFocus::new(camera_position, reference_z);
        let streamer = ChunkStreamer::new(source.world(), params, focus);
        let grid = streamer.grid();

        // Vue figée du terrain pour ce lot : le relief plus les édits du moment (aucun
        // ici, mais la génération initiale suit le même chemin que le re-maillage).
        let store = ChunkStore::default();
        let snapshot = TerrainSnapshot::new(&source, &store, debug.mesh);

        // Aucun maillage ici : le monde se construit en fond, frame après frame, pendant
        //    que le jeu tourne. Du plus proche au plus lointain de la caméra, pour que le
        //    terrain apparaisse en cercles concentriques autour du joueur.
        let mut coords = grid.coords().to_vec();
        coords.sort_by(|&a, &b| {
            chunk_distance(a, focus.pos).total_cmp(&chunk_distance(b, focus.pos))
        });
        let mut mesh_cache = MeshCache::new(params.mesh_cache_mb * 1024 * 1024);
        let ms = |v: f32| Duration::from_secs_f32(v / 1000.0);
        let frame_budget = ms(params.frame_budget_ms);
        let initial_load = MeshJob::start_initial_load(
            grid,
            coords,
            &mut mesh_cache,
            snapshot,
            ms(params.initial_load_budget_ms),
        );

        Terrain {
            source,
            store,
            chunks: InstalledChunks::new(),
            streamer,
            reference_z,
            mesh_cache,
            mesh_job: Some(initial_load),
            pending_uploads: Vec::new(),
            graveyard: Graveyard::new(frames_in_flight, frame_budget),
            visible: VisibleSet::default(),
            frame_budget,
            max_lod: params.lod.max_lod(),
            debug: debug.clone(),
            context,
            logger,
        }
    }
}

/// Stats terrain → onglet « world » du CLI, un enregistrement par niveau.
///
/// Sert de contrôle du gain LOD : on s'attend à avg(L1) ≈ avg(L0)/4 et avg(L2) ≈
/// avg(L0)/16 (la nappe est 2D : doubler le pas quadruple l'aire couverte par cellule).
/// Un peu au-dessus du ÷4 idéal en pratique — le quad de fond et les parois de bord ne
/// rétrécissent pas.
pub(super) fn log_terrain_stats(
    logger: &Logger,
    grid: &LodGrid,
    meshed: &[(IVec2, Arc<MeshData>)],
    max_lod: u8,
) {
    let levels = max_lod as usize + 1;
    let mut chunks_per = vec![0usize; levels];
    let mut verts_per = vec![0usize; levels];
    for (coord, data) in meshed {
        let lod = grid.lod(*coord) as usize;
        chunks_per[lod] += 1;
        verts_per[lod] += data.vertices.len();
    }

    // 1er enregistrement : résumé en clair (sans séparateur de champ) — le CLI l'affiche
    // tel quel, et c'est la seule trace lisible quand le moteur tourne sans CLI (le
    // logger écrit alors le message sur stderr).
    let total_chunks: usize = chunks_per.iter().sum();
    let total_verts: usize = verts_per.iter().sum();
    let mut records = vec![format!(
        "Terrain : {total_chunks} chunks, {total_verts} sommets"
    )];
    records.extend(
        (0..levels).map(|l| format!("{l}\u{1f}{}\u{1f}{}", chunks_per[l], verts_per[l])),
    );
    logger.world(&records.join("\u{1e}"));
}
