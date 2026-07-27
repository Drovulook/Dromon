use crate::app::engine::terrain_generation::{
    CHUNK_SIZE, MAX_LOD, balanced_lods, chunk_distance, mesh_chunk,
};
use crate::app::engine::{inputs::InputState, terrain_generation::ChunkManager};
use crate::app::engine::renderer::camera::Camera;
use crate::{GenParams, profile};
use anyhow::Result;
use ash::vk;
use std::sync::Arc;

use crate::app::{
    engine::{
        renderer::{
            descriptors::DescriptorHandler,
            render_resources::{RenderObject, RenderResourceManager, TerrainMesh},
        },
        rendering_context::RenderingContext,
        timer::Timer,
    },
    logger::Logger,
};
use glam::{IVec2, Vec2};
use rayon::prelude::*;

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

pub struct World {
    pub logger: Arc<Logger>,
    pub rrm: RenderResourceManager,
    pub render_objects: Vec<RenderObject>,
    pub camera: Camera,
    pub light: DirectionalLight,
    /// Données du terrain (voxels). `None` tant que la scène n'a pas appelé
    /// [`World::generate_terrain`]. Conservé pour l'édition future (creuser,
    /// poser des minerais, etc.).
    pub chunk_manager: Option<ChunkManager>,
    /// Un mesh GPU par chunk de terrain.
    pub terrain_meshes: Vec<TerrainMesh>,
    /// Contexte GPU, conservé pour pouvoir allouer les buffers du terrain au
    /// moment du `setup` (la génération est pilotée par la scène).
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
            terrain_meshes: Vec::new(),
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

        // 1) Génère TOUS les chunks (données voxel) d'abord, pour que le meshing
        //    puisse échantillonner les chunks voisins aux bords (coutures continues).
        for &coord in &coords {
            manager.generate_chunk(coord);
        }

        // 1b) LOD statique : figé une fois selon la distance horizontale du chunk à la
        //     position de départ de la caméra, puis **équilibré 2:1** — deux chunks
        //     voisins ne peuvent pas différer de plus d'un niveau, seul écart que la
        //     cellule de transition Transvoxel sait coudre. `mesh_chunk` relira ce niveau
        //     via le manager (source de vérité unique) → meshing parallèle inchangé.
        let focus = Vec2::new(self.camera.position.x, self.camera.position.y);
        let lods = balanced_lods(&coords, focus);
        for (&c, &lod) in coords.iter().zip(&lods) {
            manager.set_chunk_lod(c, lod);
        }

        // 1c) Masques de transition (Transvoxel, étape 1) : maintenant que TOUS les LOD
        //     sont fixés, on calcule pour chaque chunk quelles faces bordent un voisin
        //     plus grossier et on **stocke** le masque sur le chunk (cache dérivé, cf.
        //     `Chunk::transition_faces`). Doit venir après la boucle 1b : le masque d'un
        //     chunk dépend du LOD de ses voisins.
        for &coord in &coords {
            manager.refresh_transition_faces(coord);
        }

        // 1d) Preuve par log de la détection, sans encore générer de géométrie : combien
        //     de chunks ont au moins une face à coudre, et combien de faces au total. Les
        //     cellules de transition (qui scellent les fentes inter-LOD) viendront ensuite.
        //     Les faces se concentrent aux frontières entre anneaux de LOD.
        {
            let mut chunks_to_stitch = 0usize;
            let mut total_faces = 0usize;
            for &coord in &coords {
                let faces = manager.chunk_transition_faces(coord);
                if !faces.is_empty() {
                    chunks_to_stitch += 1;
                    total_faces += faces.iter().count();
                }
            }
            self.logger.info(&format!(
                "Transitions LOD : {chunks_to_stitch} chunks à coudre, \
                 {total_faces} faces de transition détectées"
            ));
        }

        // 2a) Meshing en parallèle : chaque chunk est indépendant et `mesh_chunk` ne
        //     lit que `&manager` (partage en lecture seule, sûr entre threads). Rayon
        //     répartit les milliers de chunks sur tous les cœurs par vol de travail.
        let meshed: Vec<(Vec<_>, Vec<_>)> = {
            profile!("mesh chunks (parallel)");
            coords
                .par_iter()
                .map(|&coord| mesh_chunk(&manager, coord))
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
            for (&lod, (v, _)) in lods.iter().zip(&meshed) {
                chunks_per[lod as usize] += 1;
                verts_per[lod as usize] += v.len();
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

        // 2b) Upload GPU séquentiel : `TerrainMesh::new` touche le contexte Vulkan et
        //     renvoie un `Result` — on le garde hors du parallélisme. On saute les
        //     chunks **vides** (buffer de taille 0 interdit par Vulkan).
        let mut terrain_meshes = Vec::with_capacity(meshed.len());
        {
            profile!("upload terrain meshes");
            for (vertices, indices) in meshed {
                if vertices.is_empty() || indices.is_empty() {
                    continue;
                }
                terrain_meshes.push(TerrainMesh::new(self.context.clone(), vertices, indices)?);
            }
        }

        self.chunk_manager = Some(manager);
        self.terrain_meshes = terrain_meshes;
        Ok(())
    }

    pub fn initialize(&self, command_buffer: &vk::CommandBuffer) -> Result<()> {
        profile!();
        self.rrm.initialize(command_buffer)?;
        for mesh in &self.terrain_meshes {
            mesh.initialize(command_buffer);
        }
        Ok(())
    }

    pub fn update_world_data(&mut self, timer: &Timer, input_state: &InputState, aspect: f32) {
        profile!();
        self.camera.update(input_state, timer, aspect);
    }
}
