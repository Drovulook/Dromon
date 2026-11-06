//! **Ce qui est réellement à l'écran** : le LOD affiché de chaque chunk, vides compris.
//!
//! Le mesh d'un chunk ne dépend que de `(coord, lod)` : ce sont les LOD **affichés** des
//! voisins, et non la configuration visée, qui décident de ses coutures. Deux voisins
//! affichés ne doivent jamais différer de plus de [`MAX_LOD_STEP`] : Transvoxel ne sait
//! pas coudre davantage. [`ShownLods::next_lod`] est le garant de cette règle.

use glam::IVec2;
use rustc_hash::FxHashMap;

use super::transition_shrink::taper_bits;
use super::{Face, MAX_LOD_STEP, TransitionFaces, transition_faces};

/// LOD affiché de chaque chunk. Un chunk absent n'a aucune géométrie à l'écran.
#[derive(Default)]
pub struct ShownLods {
    lods: FxHashMap<IVec2, u8>,
}

impl ShownLods {
    /// LOD affiché de `coord`, `None` s'il n'est pas à l'écran.
    #[inline]
    pub fn get(&self, coord: IVec2) -> Option<u8> {
        self.lods.get(&coord).copied()
    }

    pub fn set(&mut self, coord: IVec2, lod: u8) {
        self.lods.insert(coord, lod);
    }

    pub fn remove(&mut self, coord: IVec2) {
        self.lods.remove(&coord);
    }

    /// Niveau à mailler pour rapprocher `coord` de `target` : le plus proche que ses
    /// voisins affichés autorisent. `None` s'ils n'en autorisent aucun, ce qui n'arrive
    /// qu'à un chunk non affiché, entre deux voisins trop éloignés : il attend qu'ils se
    /// rapprochent. La cible étant équilibrée, quelqu'un peut toujours avancer : le plus
    /// grossier des chunks à raffiner, le plus fin des chunks à dégrader.
    pub fn next_lod(&self, coord: IVec2, target: u8) -> Option<u8> {
        let mut low = 0;
        let mut high = u8::MAX;
        for face in Face::ALL {
            if let Some(neighbor) = self.get(coord + face.offset()) {
                low = low.max(neighbor.saturating_sub(MAX_LOD_STEP));
                high = high.min(neighbor + MAX_LOD_STEP);
            }
        }
        (low <= high).then(|| target.clamp(low, high))
    }

    /// Bits de couture de `coord` (cf. `ChunkSeam`) : faces à dalle, puis atténuations.
    pub fn seam_bits(&self, coord: IVec2) -> u32 {
        let lod = |c: IVec2| self.get(c).unwrap_or(0);
        let faces = |c: IVec2| self.faces(c);
        let taper = taper_bits(coord, lod, faces);
        faces(coord).bits() as u32 | (taper as u32) << 4
    }

    /// Faces de `coord` bordant un voisin affiché plus grossier. Vide s'il est absent.
    fn faces(&self, coord: IVec2) -> TransitionFaces {
        let Some(lod) = self.get(coord) else {
            return TransitionFaces::default();
        };
        transition_faces(
            lod,
            Face::ALL.map(|face| self.get(coord + face.offset()).unwrap_or(0)),
        )
    }
}
