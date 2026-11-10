//! **Libération différée des meshes remplacés.**
//!
//! Un mesh retiré peut encore être lu par une frame en vol : ses tranches d'arène ne
//! doivent pas être réattribuées, sinon un autre chunk écraserait sa géométrie à l'écran.
//! Le cimetière le retient donc le temps que toutes les frames qui ont pu l'enregistrer
//! soient terminées.

use crate::app::engine::renderer::render_resources::TerrainMesh;
use crate::app::engine::renderer::world::terrain::arenas::TerrainArenas;
use crate::profile;

/// Meshes en attente de libération, datés de leur mise au rebut.
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

    /// Avance d'une frame et rend aux arènes les tranches que plus aucune frame en vol ne
    /// peut lire. Sans budget : quelques additions par mesh.
    pub(super) fn tick(&mut self, arenas: &mut TerrainArenas) {
        self.frame += 1;
        if self.pending.is_empty() {
            return;
        }
        profile!();
        let mut i = 0;
        while i < self.pending.len() {
            if self.frame < self.pending[i].1 + self.guard {
                i += 1;
                continue;
            }
            // L'ordre du cimetière n'a aucune importance, d'où `swap_remove`.
            let (mesh, _) = self.pending.swap_remove(i);
            mesh.release(arenas);
        }
    }
}
