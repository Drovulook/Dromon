use anyhow::Result;
use glam::{IVec2, Vec2, Vec3};
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
                        mesh_job::MeshJob, meshes::LoadedChunks,
                    },
                },
            },
            rendering_context::RenderingContext,
            terrain_generation::{
                CHUNK_SIZE, ChunkStore, LodFocus, LodGrid, LodUpdater, MAX_LOD, MeshCache,
                TerrainSnapshot, TerrainSource, chunk_distance, static_lod,
            },
        },
        logger::Logger,
    },
    profile,
};

impl World {
    /// Crée le terrain de la scène. Appelée par la scène dans `setup` ; l'upload GPU des
    /// meshes se fait ensuite dans [`World::initialize`].
    pub fn generate_terrain(&mut self, params: GenParams, radius_chunks: u32) -> Result<()> {
        // Le terrain s'étend sur tout le monde et on le survole : la boîte d'ombre
        // doit être grande et suivre la caméra.
        self.light.shadow = ShadowConfig {
            half_size: 150.0,
            near: 1.0,
            far: 1000.0,
            eye_distance: 300.0,
            follow_camera: true,
            focus_distance: 90.0,
        };

        self.terrain = Some(Terrain::generate(
            params,
            radius_chunks,
            self.camera.position,
            self.context.clone(),
            self.logger.clone(),
            self.frames_in_flight,
        )?);
        Ok(())
    }
}

impl Terrain {
    /// Génère un **disque** de chunks de terrain de rayon `radius_chunks` (exprimé en
    /// chunks) centré sur l'origine du monde, et lance son maillage en fond : le terrain
    /// rend la main vide, ses meshes apparaissent au fil des frames suivantes.
    ///
    /// Disque plutôt que carré : la distance au bord du monde ne dépend plus de la
    /// direction. À nombre de chunks égal, un carré ne garantit que `0,89 · r` dans les
    /// directions des axes, et en offre `1,25 · r` dans les diagonales — dépensés là où
    /// le joueur ne va pas plus souvent qu'ailleurs.
    fn generate(
        params: GenParams,
        radius_chunks: u32,
        camera_position: Vec3,
        context: Arc<RenderingContext>,
        logger: Arc<Logger>,
        frames_in_flight: u64,
    ) -> Result<Terrain> {
        profile!();

        let source = Arc::new(TerrainSource::new(params));

        // Coordonnées des chunks du disque : ceux dont le CENTRE tombe à moins de
        // `radius` de l'origine — même mesure que la politique de LOD, dont les anneaux
        // sont donc concentriques au bord du monde. Les centres valant `c·64 + 32`, ils
        // sont symétriques autour de 0 et la bordure `c = ±r` du carré de balayage est
        // toujours rejetée (`r·64 + 32 > r·64`).
        let r = radius_chunks as i32;
        let radius = (radius_chunks as usize * CHUNK_SIZE) as f32;
        let mut coords = Vec::with_capacity((std::f32::consts::PI * (r * r) as f32) as usize);
        for cx in -r..=r {
            for cy in -r..=r {
                let coord = IVec2::new(cx, cy);
                if chunk_distance(coord, Vec2::ZERO) <= radius {
                    coords.push(coord);
                }
            }
        }

        // Altitude moyenne du relief : plan de référence de la composante verticale du
        //    LOD. Mesurée une seule fois — le relief ne bouge pas.
        let reference_z = source.mean_terrain_height(&coords);

        // LOD initial selon la distance (horizontale ET verticale) du chunk au point de
        //    vue de départ, puis **équilibré 2:1** — deux chunks voisins ne peuvent pas
        //    différer de plus d'un niveau, seul écart que la cellule de transition
        //    Transvoxel sait coudre. `rebalance` en déduit aussi les masques de couture.
        let focus = LodFocus::new(camera_position, reference_z);
        let mut grid = LodGrid::new(coords.clone());
        grid.set_raw_lods(|coord, _| static_lod(coord, focus));
        grid.rebalance();
        let grid = Arc::new(grid);

        // Vue figée du terrain pour ce lot : le relief plus les édits du moment (aucun
        // ici, mais la génération initiale suit le même chemin que le re-maillage).
        let store = ChunkStore::default();
        let snapshot = TerrainSnapshot::new(&source, &store);

        // Aucun maillage ici : le monde se construit en fond, frame après frame, pendant
        //    que le jeu tourne. Du plus proche au plus lointain de la caméra, pour que le
        //    terrain apparaisse en cercles concentriques autour du joueur.
        let eye = camera_position.truncate();
        coords.sort_by(|&a, &b| chunk_distance(a, eye).total_cmp(&chunk_distance(b, eye)));
        let mut mesh_cache = MeshCache::default();
        let initial_load = MeshJob::start_initial_load(
            grid.clone(),
            coords,
            &mut mesh_cache,
            snapshot,
            INITIAL_LOAD_BUDGET,
        );

        Ok(Terrain {
            source,
            store,
            chunks: LoadedChunks::new(),
            lod_updater: LodUpdater::new(grid, focus),
            reference_z,
            mesh_cache,
            mesh_job: Some(initial_load),
            pending_uploads: Vec::new(),
            graveyard: Graveyard::new(frames_in_flight),
            visible: VisibleSet::default(),
            context,
            logger,
        })
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
