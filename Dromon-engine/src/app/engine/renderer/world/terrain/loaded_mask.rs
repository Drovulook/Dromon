use glam::IVec2;

/// **Masque des chunks installés**, envoyé aux shaders (`WorldMaps`, binding 2) : la mer
/// (surface, fond vu à travers, fond infini du ciel) n'est peinte qu'au-dessus d'un chunk
/// dont le terrain est affiché. Sans lui, l'eau précède le terrain en déplacement rapide
/// ou au chargement initial, et s'étend jusqu'à la portée sur des terres absentes.
///
/// Un octet par chunk (255 = installé) sur une grille `size²` en **adressage torique** :
/// le chunk `c` tombe sur le texel `c mod size`.
///
/// `size` ≥ **deux** fenêtres : les shaders lisent le masque dans le disque de portée
/// autour de la *caméra*, alors que la fenêtre chargée est centrée sur le point focal du
/// dernier lot, en retard en déplacement rapide. Avec une seule fenêtre, un point devant
/// la caméra, hors fenêtre, tomberait sur le texel d'un chunk chargé 256 chunks derrière
/// dès 2 chunks de retard (mer fantôme à l'horizon).
pub(crate) struct LoadedMask {
    size: u32,
    texels: Vec<u8>,
    /// Modifié depuis le dernier envoi au GPU.
    dirty: bool,
}

impl LoadedMask {
    /// `window_chunks` : nombre maximal de chunks de la fenêtre chargée sur un axe.
    pub fn new(window_chunks: u32) -> LoadedMask {
        // Puissance de 2 : le shader prend le modulo par un `&`, juste en négatif aussi.
        let size = (2 * window_chunks).next_power_of_two();
        LoadedMask {
            size,
            texels: vec![0; (size * size) as usize],
            // La texture naît à zéro (cf. `WorldMaps::new`) : rien à envoyer.
            dirty: false,
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn set(&mut self, coord: IVec2, installed: bool) {
        let n = self.size as i32;
        let i = (coord.y.rem_euclid(n) * n + coord.x.rem_euclid(n)) as usize;
        let value = if installed { 255 } else { 0 };
        if self.texels[i] != value {
            self.texels[i] = value;
            self.dirty = true;
        }
    }

    /// Force l'envoi au prochain `take_changes`, même sans modification : la texture
    /// porte encore le masque d'un terrain précédent (hot reload).
    pub fn force_upload(&mut self) {
        self.dirty = true;
    }

    /// Texels à envoyer au GPU, s'ils ont changé depuis le dernier appel.
    pub fn take_changes(&mut self) -> Option<&[u8]> {
        std::mem::take(&mut self.dirty).then_some(self.texels.as_slice())
    }
}
