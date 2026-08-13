use anyhow::Result;
use glam::{IVec2, Vec2};
use rayon::prelude::*;
use std::sync::Arc;

use crate::{
    GenParams, World,
    app::engine::{
        renderer::{
            light::ShadowConfig,
            render_resources::{MeshData, TerrainMesh},
        },
        terrain_generation::{
            CHUNK_SIZE, ChunkManager, LodFocus, LodGrid, LodUpdater, MAX_LOD, chunk_distance,
            mesh_chunk, static_lod,
        },
    },
    profile,
};

impl World {
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
}
