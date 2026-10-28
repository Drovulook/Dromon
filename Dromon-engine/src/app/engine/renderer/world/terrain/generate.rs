use anyhow::Result;
use glam::{IVec2, Vec3};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{
    World,
    app::{
        engine::{
            renderer::{
                render_resources::{MaterialBuffer, MeshData, WorldMaps},
                world::{
                    terrain::{
                        Terrain, culling::VisibleSet, graveyard::Graveyard,
                        loaded_mask::LoadedMask, mesh_job::MeshJob, meshes::InstalledChunks,
                    },
                    water::Water,
                },
            },
            rendering_context::RenderingContext,
            terrain_generation::{
                CHUNK_SIZE, ChunkStore, ChunkStreamer, LodFocus, LodGrid, MACRO_MAP_CACHE,
                MACRO_MAP_EXPORT_DIR, MacroMap, MeshCache, TerrainSnapshot, TerrainSource,
                chunk_distance,
            },
        },
        logger::Logger,
    },
    config::{
        DebugConfig, MaterialRulesConfig, MaterialsConfig, TerrainRenderParams, WorldGenConfig,
    },
    profile,
};

impl World {
    /// Crée le terrain de la scène. Appelée par la scène dans `setup`. Rend la main
    /// aussitôt : les chunks sont maillés en fond et apparaissent au fil des frames.
    pub fn generate_terrain(&mut self, config: &WorldGenConfig) -> Result<()> {
        // Terrain survolé : grande boîte d'ombre qui suit la caméra (`render.ron`).
        self.light.shadow = self.config.render.shadow.terrain.clone();

        let terrain = self.new_terrain(config)?;
        self.water = Some(Water::new(
            self.context.clone(),
            terrain.source.clone(),
            config.macro_.world.sea_level as f32,
            world_radius(config),
            self.water_distance(),
            &self.config.render.water,
            self.config.environment.water,
        )?);
        self.world_maps = Some(WorldMaps::new(
            self.context.clone(),
            &self.rrm.texture_handler,
            &self.descriptor_handler,
            terrain.source.macro_map(),
            terrain.loaded_mask_size(),
            self.frames_in_flight,
        )?);
        self.materials = Some(MaterialBuffer::new(
            self.context.clone(),
            &self.rrm.texture_handler,
            &self.descriptor_handler,
            &self.config.materials,
        )?);
        self.terrain = Some(terrain);
        Ok(())
    }

    /// Hot reload `Terrain` et `MacroMap` : un terrain neuf remplace l'ancien. Il repart
    /// de zéro, cache de meshes compris : les clés du cache ne contiennent pas les
    /// réglages, un mesh de l'ancien terrain y passerait pour valide.
    ///
    /// La carte macro est relue depuis son cache disque, ou recalculée (et le cache
    /// réécrit) si la config macro a changé — **bloquant** le temps du calcul. Les cartes
    /// du monde ne sont reconstruites que si elle a changé, ou si la portée de chargement
    /// change la taille du masque. La table des matériaux est gardée.
    pub(crate) fn regenerate_terrain(&mut self, config: &WorldGenConfig) -> Result<()> {
        let macro_changed = self
            .terrain
            .as_ref()
            .is_none_or(|old| old.world_gen.macro_ != config.macro_);
        let mut terrain = self.new_terrain(config)?;
        // Les frames en vol dessinent peut-être encore l'ancien terrain (buffers libérés
        // par son remplacement) et lisent les cartes du monde.
        unsafe { self.context.device.device_wait_idle()? };
        match self.world_maps.as_mut() {
            Some(world_maps)
                if macro_changed || world_maps.loaded_mask_size() != terrain.loaded_mask_size() =>
            {
                // Masque neuf, à zéro comme celui du nouveau terrain.
                world_maps.hot_reload(
                    &self.rrm.texture_handler,
                    &self.descriptor_handler,
                    terrain.source.macro_map(),
                    terrain.loaded_mask_size(),
                )?;
            }
            // Le GPU garde le masque de l'ancien terrain : on envoie celui, vide, du nouveau.
            _ => terrain.loaded_mask.force_upload(),
        }
        let water_distance = self.water_distance();
        if let Some(water) = self.water.as_mut() {
            water.hot_reload_terrain(
                terrain.source.clone(),
                config.macro_.world.sea_level as f32,
                world_radius(config),
                water_distance,
                &self.config.render.water,
            );
        }
        // L'ancien terrain est détruit ici ; ses workers encore en vol s'arrêtent au
        // premier envoi vers son canal fermé.
        self.terrain = Some(terrain);
        Ok(())
    }

    fn new_terrain(&self, config: &WorldGenConfig) -> Result<Terrain> {
        Terrain::new(
            config,
            &self.config.materials,
            &self.config.material_rules,
            &self.config.render.terrain,
            &self.config.debug,
            self.camera.position,
            self.context.clone(),
            self.logger.clone(),
            self.frames_in_flight,
        )
    }

    /// Au-delà, plus aucun chunk chargé : nœuds d'eau rejetés d'office. La coupure fine
    /// suit le masque des chunks installés (`Terrain::take_loaded_mask`).
    fn water_distance(&self) -> f32 {
        let params = &self.config.render.terrain;
        (params.load_radius_chunks as f32 + params.unload_margin_chunks) * CHUNK_SIZE as f32
    }
}

/// Rayon du monde, en unités monde.
fn world_radius(config: &WorldGenConfig) -> f32 {
    (config.macro_.world.radius_chunks as usize * CHUNK_SIZE) as f32
}

/// Écrit les PNG de la carte macro dans [`MACRO_MAP_EXPORT_DIR`] sur un thread à part
/// (~2 s en release) : le jeu continue pendant ce temps. La carte, immuable, est lue
/// via l'`Arc` du `TerrainSource`, sans copie.
///
/// Rechargements rapprochés : les exports passent un par un (deux écritures du même
/// fichier en même temps le corrompraient), et un export doublé par un plus récent
/// avant d'avoir commencé est abandonné. Les PNG finissent donc toujours sur la
/// dernière carte.
fn export_macro_pngs(source: Arc<TerrainSource>, logger: Arc<Logger>) {
    static LOCK: Mutex<()> = Mutex::new(());
    static LATEST: AtomicU64 = AtomicU64::new(0);
    let id = LATEST.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        // Un export précédent a paniqué : le verrou est « empoisonné », sans gravité ici.
        let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if LATEST.load(Ordering::SeqCst) != id {
            return;
        }
        let start = Instant::now();
        match source
            .macro_map()
            .export_pngs(Path::new(MACRO_MAP_EXPORT_DIR))
        {
            Ok(_) => logger.info(&format!(
                "PNG de la carte macro écrits dans {MACRO_MAP_EXPORT_DIR} en {:.1} s",
                start.elapsed().as_secs_f32()
            )),
            Err(e) => logger.warn(&format!("export PNG de la carte macro : {e:#}")),
        }
    });
}

impl Terrain {
    /// Crée le monde et lance le maillage en fond de la fenêtre chargée initiale : le
    /// terrain rend la main vide, ses meshes apparaissent au fil des frames suivantes.
    fn new(
        config: &WorldGenConfig,
        materials: &MaterialsConfig,
        material_rules: &MaterialRulesConfig,
        params: &TerrainRenderParams,
        debug: &DebugConfig,
        camera_position: Vec3,
        context: Arc<RenderingContext>,
        logger: Arc<Logger>,
        frames_in_flight: u64,
    ) -> Result<Terrain> {
        profile!();

        // Relue depuis le cache (cf. exemple `macro_map`), recalculée s'il est périmé.
        let (macro_map, rebuilt) = MacroMap::load_or_build(config, Path::new(MACRO_MAP_CACHE))?;
        let source = Arc::new(TerrainSource::new(
            config,
            materials,
            material_rules,
            macro_map,
        )?);
        // Relue du cache, la carte est celle des PNG déjà écrits : rien à exporter.
        if rebuilt {
            export_macro_pngs(source.clone(), logger.clone());
        }

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

        Ok(Terrain {
            world_gen: config.clone(),
            source,
            store,
            chunks: InstalledChunks::new(),
            // Fenêtre : disque de rayon `portée + marge` chunks, soit au plus
            // ⌈2·rayon⌉ + 1 chunks sur un axe.
            loaded_mask: LoadedMask::new(
                (2.0 * (params.load_radius_chunks as f32 + params.unload_margin_chunks)).ceil()
                    as u32
                    + 1,
            ),
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
    records
        .extend((0..levels).map(|l| format!("{l}\u{1f}{}\u{1f}{}", chunks_per[l], verts_per[l])));
    logger.world(&records.join("\u{1e}"));
}
