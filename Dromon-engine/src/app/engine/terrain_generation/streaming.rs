//! **Streaming des chunks.** Décide quels chunks sont *chargés* (fenêtre circulaire
//! autour de la caméra, coupée par le bord du monde) et à quel LOD, et dit lesquels
//! retirer quand la caméra a assez bougé. Rapprocher l'écran de cette cible est le
//! travail de la file de maillage.
//!
//! ## Monde ≠ chargé
//! Le **monde** est un disque fixe ([`WorldDisc`]) : c'est lui qui porte les murs de
//! bordure. La **fenêtre chargée** le suit à la caméra et ne produit aucun mur : un chunk
//! au bord de la fenêtre n'est pas au bord du monde.
//!
//! ## Filtrage, du plus fréquent au plus cher
//! ```text
//! chaque frame → focus.moved_sq(last) > seuil²             (~5 ns, jamais de sqrt)
//! si franchi   → fenêtre + LOD + hystérésis + équilibrage
//! puis         → cible changée ? sortants ? → mise à jour
//! ```
//!
//! ## Le total ne dépend pas du seuil
//! Augmenter `MOVE_THRESHOLD` espace les recalculs : le nombre de chunks qui changent par
//! unité de distance parcourue reste le même. Le seuil ne sert qu'à éviter de refaire le
//! calcul à chaque frame.

use super::chunk::{CHUNK_SIZE, WorldDisc};
use super::lod::{LodFocus, chunk_distance, grid::LodGrid, hysteretic_lod, static_lod};
use crate::config::{LodParams, TerrainRenderParams};
use crate::profile;
use glam::{IVec2, Vec2};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Déplacement du point focal (unités monde) à partir duquel on recalcule fenêtre et LOD.
/// L'ordre de grandeur du demi-chunk : plus fin ne change rien au résultat visible,
/// beaucoup plus grossier retarderait le terrain.
pub const MOVE_THRESHOLD: f32 = 32.0;

/// Intervalle minimal entre deux recalculs. Le seuil de distance seul en ferait ~47 par
/// seconde à 1 500 u/s, chacun parcourant toute la fenêtre.
const MIN_UPDATE_PERIOD: Duration = Duration::from_millis(100);

// Hystérésis du chargement : `TerrainRenderParams::unload_margin_chunks`. Marge absolue et
// non relative comme pour le LOD : sur un rayon de 5 000 unités, 8 % feraient ~6 chunks
// de marge, soit ~17 % de chunks en trop.

/// Une nouvelle cible : la configuration visée et les chunks à retirer.
pub struct StreamUpdate {
    /// Configuration cible (chunks chargés + LOD).
    pub grid: Arc<LodGrid>,
    /// Chunks sortis de la fenêtre : leur mesh est à retirer.
    pub removed: Vec<IVec2>,
}

/// Garde la configuration courante et le dernier point focal évalué.
pub struct ChunkStreamer {
    grid: Arc<LodGrid>,
    last_focus: LodFocus,
    /// Date du dernier recalcul, pour [`MIN_UPDATE_PERIOD`].
    last_update: Instant,
    threshold_sq: f32,
    world: WorldDisc,
    /// Rayon de chargement (unités monde).
    load_radius: f32,
    /// Marge de déchargement (unités monde) au-delà de `load_radius`.
    unload_margin: f32,
    /// Politique distance → LOD.
    lod: LodParams,
}

impl ChunkStreamer {
    /// Calcule la fenêtre initiale autour de `focus` (typiquement la position de départ
    /// de la caméra). Le chargement initial la récupère via [`ChunkStreamer::grid`].
    pub fn new(world: WorldDisc, params: &TerrainRenderParams, focus: LodFocus) -> ChunkStreamer {
        let chunk = CHUNK_SIZE as f32;
        let mut streamer = ChunkStreamer {
            grid: Arc::new(LodGrid::new(Vec::new())),
            last_focus: focus,
            last_update: Instant::now(),
            threshold_sq: MOVE_THRESHOLD * MOVE_THRESHOLD,
            world,
            load_radius: params.load_radius_chunks as f32 * chunk,
            unload_margin: params.unload_margin_chunks * chunk,
            lod: params.lod.clone(),
        };
        // Depuis une grille vide : tous les chunks sont entrants, LOD sans hystérésis.
        streamer.grid = Arc::new(streamer.next_grid(focus));
        streamer
    }

    /// Configuration courante.
    pub fn grid(&self) -> Arc<LodGrid> {
        self.grid.clone()
    }

    /// Réévalue fenêtre et LOD si le point focal a franchi le seuil. Renvoie la nouvelle
    /// cible, ou `None` si rien n'a bougé — ou si le nouveau calcul ne change aucun LOD
    /// visé (cas fréquent : la caméra glisse sans faire basculer un seul chunk).
    pub fn update(&mut self, focus: LodFocus) -> Option<StreamUpdate> {
        if focus.moved_sq(self.last_focus) <= self.threshold_sq
            || self.last_update.elapsed() < MIN_UPDATE_PERIOD
        {
            return None;
        }
        profile!();
        self.last_focus = focus;
        self.last_update = Instant::now();

        let next = self.next_grid(focus);
        let changed = next.lods_differ(&self.grid);
        let removed: Vec<IVec2> = self
            .grid
            .coords()
            .iter()
            .copied()
            .filter(|&c| !next.contains(c))
            .collect();

        // On adopte la nouvelle grille même sans LOD changé : les niveaux BRUTS ont pu bouger
        // (état de l'hystérésis). Ne pas les garder ferait repartir l'hystérésis d'un état
        // périmé à la passe suivante.
        self.grid = Arc::new(next);

        (changed || !removed.is_empty()).then(|| StreamUpdate {
            grid: self.grid.clone(),
            removed,
        })
    }

    /// Grille **recentrée** sur `focus` : nouvelle fenêtre, LOD bruts repris de la grille
    /// courante (hystérésis) ou calculés à neuf pour les entrants, puis équilibrage.
    ///
    /// On reconstruit plutôt que de décaler en place : `LodGrid::new` recalcule sa boîte
    /// englobante depuis les coordonnées.
    fn next_grid(&self, focus: LodFocus) -> LodGrid {
        let mut next = LodGrid::new(self.window(focus.pos));
        let prev = &self.grid;
        next.set_raw_lods(|c, _| match prev.raw(c) {
            Some(raw) => hysteretic_lod(c, focus, raw, &self.lod),
            None => static_lod(c, focus, &self.lod),
        });
        next.rebalance();
        next
    }

    /// Chunks du monde à garder chargés autour de `center` : entrants sous
    /// `load_radius`, déjà chargés jusqu'à `load_radius + unload_margin`.
    fn window(&self, center: Vec2) -> Vec<IVec2> {
        let reach = self.load_radius + self.unload_margin;
        let size = CHUNK_SIZE as f32;
        let lo = ((center - reach) / size).floor().as_ivec2();
        let hi = ((center + reach) / size).floor().as_ivec2();

        let mut coords = Vec::new();
        for cy in lo.y..=hi.y {
            for cx in lo.x..=hi.x {
                let c = IVec2::new(cx, cy);
                if !self.world.contains(c) {
                    continue;
                }
                let radius = if self.grid.contains(c) {
                    reach
                } else {
                    self.load_radius
                };
                if chunk_distance(c, center) <= radius {
                    coords.push(c);
                }
            }
        }
        coords
    }
}
