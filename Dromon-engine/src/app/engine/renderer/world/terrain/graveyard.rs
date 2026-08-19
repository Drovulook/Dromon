//! **Destruction différée des meshes remplacés.**
//!
//! `Buffer::drop` appelle `vkDestroyBuffer` + `vkFreeMemory` **immédiatement** : libérer
//! un mesh encore référencé par une frame en vol produirait une erreur de validation,
//! voire un crash. Le cimetière retient donc les meshes retirés le temps que toutes les
//! frames qui ont pu les enregistrer soient terminées.

use std::time::Instant;

use crate::app::engine::renderer::render_resources::TerrainMesh;
use crate::app::engine::renderer::world::terrain::FRAME_BUDGET;
use crate::profile;

/// Meshes en attente de destruction, datés de leur mise au rebut.
pub(super) struct Graveyard {
    pending: Vec<(TerrainMesh, u64)>,
    /// Compteur de frames, avancé par [`Graveyard::tick`]. C'est la seule horloge du
    /// terrain : rien d'autre n'a besoin de dater quoi que ce soit.
    frame: u64,
    /// Nombre de frames à attendre avant qu'un mesh soit sûrement libérable.
    guard: u64,
}

impl Graveyard {
    /// Un mesh est retiré à la frame `f` **avant** l'enregistrement du command buffer de
    /// `f` : seules les frames `< f` ont pu le référencer. Attendre `frames_in_flight + 1`
    /// frames laisse à toutes le temps d'être signalées, avec une frame de marge (le
    /// rebut a lieu avant le `wait_for_fences` de la frame courante).
    pub(super) fn new(frames_in_flight: u64) -> Graveyard {
        Graveyard {
            pending: Vec::new(),
            frame: 0,
            guard: frames_in_flight + 1,
        }
    }

    /// Met un mesh au rebut, daté de la frame courante.
    pub(super) fn bury(&mut self, mesh: TerrainMesh) {
        self.pending.push((mesh, self.frame));
    }

    /// Avance d'une frame et détruit ce que plus aucune frame en vol ne peut lire,
    /// **sous le même budget de temps** que la construction : 4 opérations noyau par
    /// mesh — libérer 200 anciens meshes d'un coup coûterait exactement le pic qu'on a
    /// éliminé côté construction. Garder un mesh mort quelques frames de plus ne coûte
    /// que de la mémoire.
    pub(super) fn tick(&mut self) {
        self.frame += 1;
        if self.pending.is_empty() {
            return;
        }
        profile!();
        let deadline = Instant::now() + FRAME_BUDGET;

        let mut i = 0;
        while i < self.pending.len() {
            if self.frame < self.pending[i].1 + self.guard {
                i += 1;
                continue;
            }
            // `swap_remove` rend l'élément, qui est droppé ici : c'est la destruction
            // Vulkan effective. L'ordre du cimetière n'a aucune importance.
            self.pending.swap_remove(i);
            if Instant::now() >= deadline {
                return;
            }
        }
    }
}
