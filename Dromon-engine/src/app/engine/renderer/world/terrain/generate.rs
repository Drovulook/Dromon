use anyhow::Result;
use glam::{IVec2, Vec3};
use std::sync::Arc;

use crate::{
    GenParams, World,
    app::{
        engine::{
            renderer::{
                render_resources::MeshData,
                world::{
                    light::ShadowConfig,
                    terrain::{
                        INITIAL_LOAD_BUDGET, Terrain, culling::VisibleSet, graveyard::Graveyard,
                        mesh_job::MeshJob, meshes::InstalledChunks,
                    },
                },
            },
            rendering_context::RenderingContext,
            terrain_generation::{
                ChunkStore, ChunkStreamer, LodFocus, LodGrid, MAX_LOD, MeshCache, TerrainSnapshot,
                TerrainSource, chunk_distance,
            },
        },
        logger::Logger,
    },
    profile,
};

impl World {
    /// Crée le terrain de la scène. Appelée par la scène dans `setup`. Rend la main
    /// aussitôt : les chunks sont maillés en fond et apparaissent au fil des frames.
    ///
    /// `load_radius_chunks` = portée du streaming autour de la caméra ; l'étendue du
    /// monde, elle, est dans `params.world_radius`.
    pub fn generate_terrain(&mut self, params: GenParams, load_radius_chunks: u32) -> Result<()> {
        // Le terrain s'étend sur tout le monde et on le survole : la boîte d'ombre
        // doit être grande et suivre la caméra.
        // Boîte de 4000 unités (~2 u/texel) : couvre une vallée entière devant la
        // caméra, au prix d'ombres plus floues de près. L'œil recule de 3000 le long
        // des rayons pour passer au-dessus des plus hauts sommets (relief ≤ ~1300,
        // lumière à ~30° de la verticale) ; `far` couvre ensuite tout le relief,
        // inclinaison de la boîte comprise (4000 · tan 30° ≈ 2300 de plus).
        self.light.shadow = ShadowConfig {
            half_size: 2000.0,
            near: 1.0,
            far: 8000.0,
            eye_distance: 3000.0,
            follow_camera: true,
            focus_distance: 1500.0,
        };

        self.terrain = Some(Terrain::new(
            params,
            load_radius_chunks,
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
        params: GenParams,
        load_radius_chunks: u32,
        camera_position: Vec3,
        context: Arc<RenderingContext>,
        logger: Arc<Logger>,
        frames_in_flight: u64,
    ) -> Terrain {
        profile!();

        let source = Arc::new(TerrainSource::new(params));

        // Altitude moyenne du relief : plan de référence de la composante verticale du
        //    LOD. Mesurée une seule fois — le relief ne bouge pas.
        let reference_z = source.mean_terrain_height();

        // Fenêtre chargée initiale et ses LOD (distance horizontale ET verticale au
        //    point de vue de départ, équilibrés 2:1 — cf. `LodGrid::rebalance`).
        let focus = LodFocus::new(camera_position, reference_z);
        let streamer = ChunkStreamer::new(source.world(), load_radius_chunks, focus);
        let grid = streamer.grid();

        // Vue figée du terrain pour ce lot : le relief plus les édits du moment (aucun
        // ici, mais la génération initiale suit le même chemin que le re-maillage).
        let store = ChunkStore::default();
        let snapshot = TerrainSnapshot::new(&source, &store);

        // Aucun maillage ici : le monde se construit en fond, frame après frame, pendant
        //    que le jeu tourne. Du plus proche au plus lointain de la caméra, pour que le
        //    terrain apparaisse en cercles concentriques autour du joueur.
        let mut coords = grid.coords().to_vec();
        coords.sort_by(|&a, &b| {
            chunk_distance(a, focus.pos).total_cmp(&chunk_distance(b, focus.pos))
        });
        let mut mesh_cache = MeshCache::default();
        let initial_load = MeshJob::start_initial_load(
            grid,
            coords,
            &mut mesh_cache,
            snapshot,
            INITIAL_LOAD_BUDGET,
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
            graveyard: Graveyard::new(frames_in_flight),
            visible: VisibleSet::default(),
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
) {
    let mut chunks_per = [0usize; MAX_LOD as usize + 1];
    let mut verts_per = [0usize; MAX_LOD as usize + 1];
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
        (0..=MAX_LOD as usize).map(|l| format!("{l}\u{1f}{}\u{1f}{}", chunks_per[l], verts_per[l])),
    );
    logger.world(&records.join("\u{1e}"));
}
