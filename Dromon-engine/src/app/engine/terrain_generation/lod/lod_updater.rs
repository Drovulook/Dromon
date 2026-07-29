//! **Suivi du LOD par la caméra.** Recalcule la [`LodGrid`] quand le point de vue a
//! assez bougé, et dit quels chunks doivent être re-maillés.
//!
//! ## Quatre étages de filtrage, du plus fréquent au plus cher
//! ```text
//! chaque frame → focus.moved_sq(last) > seuil²          (~5 ns, jamais de sqrt)
//! si franchi   → recalcul LOD + hystérésis + équilibrage (~0,2 ms)
//! puis         → diff (lod, faces) → liste des chunks sales
//! si non vide  → lot envoyé au re-maillage de fond
//! ```
//!
//! ## Le total ne dépend pas du seuil
//! Augmenter `MOVE_THRESHOLD` fait des lots plus gros mais plus rares : le nombre de
//! chunks re-maillés par unité de distance parcourue est le même. Le seuil ne sert qu'à
//! éviter de refaire le calcul de LOD à chaque frame.

use super::{LodFocus, grid::LodGrid, hysteretic_lod};
use glam::IVec2;
use std::sync::Arc;

/// Déplacement du point focal (unités monde) à partir duquel on recalcule les LOD.
/// L'ordre de grandeur du demi-chunk : plus fin ne change rien au résultat visible,
/// beaucoup plus grossier ferait des lots inutilement gros.
pub const MOVE_THRESHOLD: f32 = 32.0;

/// Un lot de travail : la configuration LOD visée et les chunks qui doivent être
/// re-maillés pour l'atteindre.
///
/// La `grid` voyage en `Arc` jusqu'aux threads de maillage : chaque lot maille contre
/// une configuration **figée**, même si la caméra en produit une nouvelle entre-temps.
pub struct LodUpdate {
    /// Configuration LOD cible, partagée avec les workers.
    pub grid: Arc<LodGrid>,
    /// Chunks dont `(lod, faces)` a changé — donc dont la géométrie change.
    pub dirty: Vec<IVec2>,
}

/// Garde la configuration LOD courante et le dernier point focal évalué.
pub struct LodUpdater {
    grid: Arc<LodGrid>,
    last_focus: LodFocus,
    threshold_sq: f32,
}

impl LodUpdater {
    /// Part d'une grille déjà calculée et du point focal qui l'a produite (typiquement
    /// la position initiale de la caméra, cf. `World::generate_terrain`).
    pub fn new(grid: LodGrid, focus: LodFocus) -> LodUpdater {
        LodUpdater {
            grid: Arc::new(grid),
            last_focus: focus,
            threshold_sq: MOVE_THRESHOLD * MOVE_THRESHOLD,
        }
    }

    /// Réévalue le LOD si le point focal a franchi le seuil. Renvoie le lot à mailler,
    /// ou `None` si rien n'a bougé — ou si le nouveau calcul ne change aucune géométrie
    /// (cas fréquent : la caméra glisse sans faire basculer un seul chunk d'anneau).
    ///
    /// ⚠ **N'appeler que si aucun lot n'est en vol.** Le lot renvoyé est le seul chemin
    /// vers la nouvelle configuration : le jeter reviendrait à afficher une géométrie qui
    /// ne correspond plus à `self.grid`. L'appelant garde donc le lot jusqu'au commit.
    pub fn update(&mut self, focus: LodFocus) -> Option<LodUpdate> {
        if focus.moved_sq(self.last_focus) <= self.threshold_sq {
            return None;
        }
        self.last_focus = focus;

        // Copie de travail (~40 Ko) : la version courante reste intacte tant que le
        // nouveau lot n'est pas commité, et sert de référence au diff.
        let mut next = (*self.grid).clone();
        next.set_raw_lods(|coord, raw| hysteretic_lod(coord, focus, raw));
        next.rebalance();
        let dirty = next.dirty_against(&self.grid);

        // On adopte la nouvelle grille même quand `dirty` est vide : les niveaux BRUTS
        // ont pu bouger (état de l'hystérésis) sans que `(lod, faces)` change. Ne pas
        // les garder ferait repartir l'hystérésis d'un état périmé à la passe suivante.
        self.grid = Arc::new(next);

        if dirty.is_empty() {
            None
        } else {
            Some(LodUpdate {
                grid: self.grid.clone(),
                dirty,
            })
        }
    }
}
