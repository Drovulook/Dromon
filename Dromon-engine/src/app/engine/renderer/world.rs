use crate::app::engine::renderer::camera::Camera;
use crate::app::engine::terrain_generation::{
    CHUNK_SIZE, LodFocus, LodGrid, LodUpdater, MAX_LOD, MeshCache, MeshKey, chunk_distance,
    mesh_chunk, static_lod,
};
use crate::app::engine::{inputs::InputState, terrain_generation::ChunkManager};
use crate::{GenParams, profile};
use anyhow::Result;
use ash::vk;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use crate::app::{
    engine::{
        renderer::{
            descriptors::DescriptorHandler,
            render_resources::{MeshData, RenderObject, RenderResourceManager, TerrainMesh},
        },
        rendering_context::RenderingContext,
        timer::Timer,
    },
    logger::Logger,
};
use glam::{IVec2, Vec2};
use rayon::prelude::*;
use std::time::{Duration, Instant};

/// Temps que l'on s'autorise **par frame** à bâtir les buffers Vulkan d'un lot, et autant
/// à détruire les anciens. Un budget en temps plutôt qu'en nombre de meshes : le coût
/// d'une allocation Vulkan varie fortement selon le driver et la taille du chunk, alors
/// qu'une limite en millisecondes s'ajuste d'elle-même. Au moins un mesh est toujours
/// traité, pour garantir la progression même si le budget est déjà dépassé.
///
/// 2 ms sur les 16 d'une frame à 60 fps : le lot met quelques frames de plus à
/// apparaître, ce qui ne se voit pas ; une image de 200 ms, si.
const BUILD_BUDGET: Duration = Duration::from_millis(2);

// Paramètres du « frustum » orthographique de la lumière (la boîte qui doit
// englober toute la scène projetant des ombres). La boîte SUIT la caméra
// (cf. `view_proj`) ; ces valeurs fixent sa taille, pas sa position. Monde Z-up.
//
// Compromis résolution : la shadow map (2048²) est étalée sur `2*HALF_SIZE`
// unités → ~`2*HALF_SIZE / 2048` u/texel. Plus la boîte est grande, plus on
// couvre de terrain mais plus les ombres deviennent grossières. ~150 ⇒ ~0.15
// u/texel, correct à l'échelle voxel.
/// Réglages du « frustum » orthographique de la shadow map. Portés par la scène
/// (via [`DirectionalLight`]) car l'échelle dépend du contenu : une petite scène
/// d'objets fixes et un terrain géant qu'on survole ne veulent ni la même taille
/// de boîte, ni la même stratégie de centrage.
///
/// La shadow map (2048²) est étalée sur `2 * half_size` unités → résolution
/// `2 * half_size / 2048` u/texel : plus la boîte est grande, plus on couvre de
/// monde mais plus les ombres deviennent grossières.
pub struct ShadowConfig {
    /// Demi-largeur/hauteur de la boîte orthographique, en unités monde.
    pub half_size: f32,
    /// Plans near/far le long de l'axe lumière, depuis l'« œil » virtuel.
    /// ⚠ `far` élargit aussi le biais anti-acné (exprimé en profondeur
    /// normalisée) : un `far` énorme sur de petits objets décolle leur ombre.
    pub near: f32,
    pub far: f32,
    /// Recul de l'« œil » virtuel le long de `-direction`.
    pub eye_distance: f32,
    /// Si `true`, la boîte suit la caméra (indispensable pour un grand terrain) ;
    /// sinon elle reste centrée sur l'origine (idéal pour une scène d'objets
    /// fixes autour de l'origine).
    pub follow_camera: bool,
    /// Quand `follow_camera`, distance devant la caméra (le long du regard) du
    /// point de centrage : on dépense le budget d'ombre là où le joueur regarde.
    pub focus_distance: f32,
}

impl Default for ShadowConfig {
    /// Défauts pour une **petite scène statique** centrée sur l'origine
    /// (p. ex. model_sandbox). `generate_terrain` les remplace par des valeurs
    /// adaptées au terrain.
    fn default() -> Self {
        ShadowConfig {
            half_size: 15.0,
            near: 0.1,
            far: 60.0,
            eye_distance: 30.0,
            follow_camera: false,
            focus_distance: 0.0,
        }
    }
}

pub struct DirectionalLight {
    pub direction: glam::Vec3,
    pub color: glam::Vec3,
    pub intensity: f32,
    pub shadow: ShadowConfig,
}

impl DirectionalLight {
    /// Matrice view*proj de la lumière : on place une caméra orthographique le
    /// long de la direction du soleil, regardant le centre de la boîte d'ombre.
    /// C'est l'équivalent de `camera.view * camera.proj`, mais pour la lumière,
    /// et en projection orthographique (rayons parallèles = pas de perspective).
    ///
    /// Le centre dépend de [`ShadowConfig::follow_camera`] : soit l'origine
    /// (scène statique), soit un point devant la caméra (terrain) pour que la
    /// zone ombrée suive le joueur — `cam_pos`/`cam_front` servent à ce calcul.
    ///
    /// Note : volontairement PAS de flip de l'axe Y (contrairement à la caméra).
    /// La shadow map est rasterisée ET échantillonnée avec cette même matrice,
    /// donc le résultat reste cohérent ; inverser Y ici ne ferait que retourner
    /// la texture sans rien changer au calcul d'ombre.
    pub fn view_proj(&self, cam_pos: glam::Vec3, cam_front: glam::Vec3) -> glam::Mat4 {
        profile!();
        let dir = self.direction.normalize();
        let s = &self.shadow;

        let center = if s.follow_camera {
            cam_pos + cam_front * s.focus_distance
        } else {
            glam::Vec3::ZERO
        };
        let eye = center - dir * s.eye_distance;

        // « up » de la caméra-lumière : Z-up en général, sauf si la lumière est
        // quasi verticale (colinéaire à Z) — on bascule alors sur Y pour éviter
        // un look_at dégénéré.
        let up = if dir.cross(glam::Vec3::Z).length_squared() < 1e-4 {
            glam::Vec3::Y
        } else {
            glam::Vec3::Z
        };

        let view = glam::Mat4::look_at_rh(eye, center, up);
        // orthographic_rh (et non _gl) : profondeur clippée dans [0, 1], la
        // convention attendue par Vulkan.
        let proj = glam::Mat4::orthographic_rh(
            -s.half_size,
            s.half_size,
            -s.half_size,
            s.half_size,
            s.near,
            s.far,
        );
        proj * view
    }
}

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
struct MeshJob {
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

pub struct World {
    pub logger: Arc<Logger>,
    pub rrm: RenderResourceManager,
    pub render_objects: Vec<RenderObject>,
    pub camera: Camera,
    pub light: DirectionalLight,
    /// Données du terrain (relief + édits). `None` tant que la scène n'a pas appelé
    /// [`World::generate_terrain`]. En `Arc` car **immuable une fois généré** : les
    /// threads de maillage en partagent la lecture sans copie ni verrou.
    pub chunk_manager: Option<Arc<ChunkManager>>,
    /// Un mesh GPU par chunk **non vide**, indexé par coordonnée : c'est ce qui permet
    /// de remplacer le mesh d'un chunk précis quand son LOD change (un `Vec` n'avait
    /// aucun lien avec les positions, les chunks vides décalant même les index).
    pub terrain_meshes: HashMap<IVec2, TerrainMesh>,
    /// Politique de LOD suivant la caméra. `None` tant qu'il n'y a pas de terrain.
    lod_updater: Option<LodUpdater>,
    /// Altitude moyenne du relief : plan de référence de la composante « hauteur de
    /// caméra » du LOD (cf. [`LodFocus`]).
    terrain_reference_z: f32,
    /// Géométries déjà calculées, gardées pour les allers-retours de caméra.
    mesh_cache: MeshCache,
    /// Lot de re-maillage en cours, s'il y en a un.
    mesh_job: Option<MeshJob>,
    /// Chunks dont les buffers attendent leur copie staging → device.
    pending_uploads: Vec<IVec2>,
    /// Meshes remplacés, avec la frame de mise au rebut. `Buffer::drop` détruit le
    /// buffer Vulkan **immédiatement** : libérer tout de suite un mesh encore
    /// référencé par une frame en vol produirait une erreur de validation, voire un
    /// crash. On attend donc que toutes les frames concernées soient terminées.
    graveyard: Vec<(TerrainMesh, u64)>,
    /// Compteur de frames, base du délai du cimetière.
    frame: u64,
    /// Nombre de frames que le renderer garde en vol.
    frames_in_flight: u64,
    /// Contexte GPU, conservé pour pouvoir allouer les buffers du terrain au
    /// moment du `setup` (la génération est pilotée par la scène) comme en cours de jeu.
    context: Arc<RenderingContext>,
}

impl World {
    /// Crée un monde *vide* : seul le gestionnaire de ressources (`rorm`) et des
    /// valeurs par défaut (caméra, lumière) sont initialisés. Le contenu concret
    /// (assets + `RenderObject`, terrain) est fourni par la `Scene` via
    /// `Scene::setup`, appelée juste après la construction du `Renderer`. La
    /// scène peut aussi modifier `world.light` à ce moment-là.
    pub fn new(
        logger: Arc<Logger>,
        context: Arc<RenderingContext>,
        descriptor_handler: Arc<DescriptorHandler>,
        frames_in_flight: usize,
    ) -> Result<World> {
        let rorm = RenderResourceManager::new(context.clone(), logger.clone(), descriptor_handler)?;

        Ok(World {
            logger,
            rrm: rorm,
            render_objects: Vec::new(),
            camera: Camera::default(),
            light: DirectionalLight {
                direction: glam::Vec3::new(-0.3, -0.5, -1.0),
                color: glam::Vec3::ONE,
                intensity: 1.0,
                // Défaut « petite scène » : boîte fixe à l'origine. `generate_terrain`
                // bascule en mode terrain si la scène crée un terrain.
                shadow: ShadowConfig::default(),
            },
            chunk_manager: None,
            terrain_meshes: HashMap::new(),
            lod_updater: None,
            terrain_reference_z: 0.0,
            mesh_cache: MeshCache::default(),
            mesh_job: None,
            pending_uploads: Vec::new(),
            graveyard: Vec::new(),
            frame: 0,
            frames_in_flight: frames_in_flight as u64,
            context,
        })
    }

    /// Génère un **disque** de chunks de terrain de rayon `radius_chunks` (exprimé en
    /// chunks) centré sur l'origine du monde, et construit un `TerrainMesh` par chunk.
    /// Appelée par la scène dans `setup` ; l'upload GPU des meshes se fait ensuite
    /// dans [`World::initialize`].
    ///
    /// Disque plutôt que carré : la distance au bord du monde ne dépend plus de la
    /// direction. À nombre de chunks égal, un carré ne garantit que `0,89 · r` dans les
    /// directions des axes, et en offre `1,25 · r` dans les diagonales — dépensés là où
    /// le joueur ne va pas plus souvent qu'ailleurs.
    pub fn generate_terrain(&mut self, params: GenParams, radius_chunks: u32) -> Result<()> {
        profile!();
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

        let mut manager = ChunkManager::new(params);

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

        // Génère TOUS les chunks (données voxel) d'abord, pour que le meshing
        //    puisse échantillonner les chunks voisins aux bords (coutures continues).
        for &coord in &coords {
            manager.generate_chunk(coord);
        }

        // Altitude moyenne du relief : plan de référence de la composante verticale du
        //    LOD. Mesurée une seule fois — le relief ne bouge pas.
        self.terrain_reference_z = manager.mean_terrain_height(&coords);

        // LOD initial selon la distance (horizontale ET verticale) du chunk au point de
        //    vue de départ, puis **équilibré 2:1** — deux chunks voisins ne peuvent pas
        //    différer de plus d'un niveau, seul écart que la cellule de transition
        //    Transvoxel sait coudre. `rebalance` en déduit aussi les masques de couture.
        let focus = LodFocus::new(self.camera.position, self.terrain_reference_z);
        let mut grid = LodGrid::new(coords.clone());
        grid.set_raw_lods(|coord, _| static_lod(coord, focus));
        grid.rebalance();

        let manager = Arc::new(manager);

        // Meshing en parallèle : chaque chunk est indépendant et `mesh_chunk` ne lit que
        //    des références partagées (sûr entre threads). Rayon répartit les milliers de
        //    chunks sur tous les cœurs par vol de travail.
        let meshed: Vec<(IVec2, MeshData)> = {
            profile!("mesh chunks (parallel)");
            coords
                .par_iter()
                .map(|&coord| (coord, mesh_chunk(&manager, &grid, coord)))
                .collect()
        };

        // Stats terrain → onglet « world » du CLI, un enregistrement par niveau.
        // Sert de contrôle du gain LOD : on s'attend à avg(L1) ≈ avg(L0)/4 et
        // avg(L2) ≈ avg(L0)/16 (la nappe est 2D : doubler le pas quadruple l'aire
        // couverte par cellule). Un peu au-dessus du ÷4 idéal en pratique — le quad
        // de fond et les parois de bord ne rétrécissent pas.
        {
            let mut chunks_per = [0usize; MAX_LOD as usize + 1];
            let mut verts_per = [0usize; MAX_LOD as usize + 1];
            for (coord, data) in &meshed {
                let lod = grid.lod(*coord) as usize;
                chunks_per[lod] += 1;
                verts_per[lod] += data.vertices.len();
            }

            // 1er enregistrement : résumé en clair (sans séparateur de champ) — le CLI
            // l'affiche tel quel, et c'est la seule trace lisible quand le moteur tourne
            // sans CLI (le logger écrit alors le message sur stderr).
            let total_chunks: usize = chunks_per.iter().sum();
            let total_verts: usize = verts_per.iter().sum();
            let mut records = vec![format!(
                "Terrain : {total_chunks} chunks, {total_verts} sommets"
            )];
            records.extend(
                (0..=MAX_LOD as usize)
                    .map(|l| format!("{l}\u{1f}{}\u{1f}{}", chunks_per[l], verts_per[l])),
            );
            self.logger.world(&records.join("\u{1e}"));
        }

        // Upload GPU séquentiel : `TerrainMesh::new` touche le contexte Vulkan et
        //    renvoie un `Result` — on le garde hors du parallélisme. On saute les chunks
        //    **vides** (buffer de taille 0 interdit par Vulkan), mais on les met quand
        //    même au cache : ça évite de les re-mailler pour rien.
        {
            profile!("upload terrain meshes");
            for (coord, data) in meshed {
                let data = Arc::new(data);
                let key = Self::mesh_key(&grid, coord);
                self.mesh_cache.insert(key, data.clone());
                if data.is_empty() {
                    continue;
                }
                self.terrain_meshes
                    .insert(coord, TerrainMesh::new(self.context.clone(), data)?);
            }
        }

        self.lod_updater = Some(LodUpdater::new(grid, focus));
        self.chunk_manager = Some(manager);
        Ok(())
    }

    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        profile!();
        self.rrm.initialize(command_buffer)?;
        for mesh in self.terrain_meshes.values() {
            mesh.record_upload(command_buffer);
        }
        Ok(())
    }

    pub fn update_world_data(&mut self, timer: &Timer, input_state: &InputState, aspect: f32) {
        profile!();
        self.camera.update(input_state, timer, aspect);
    }

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
    fn mesh_key(grid: &LodGrid, coord: IVec2) -> MeshKey {
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
