use crate::{HeightParams, app::engine::terrain_generation::generation::height_field::HeightField};
use glam::IVec2;

use super::CHUNK_SIZE;

/// Paramètres de génération du relief : graine + forme du champ d'altitude.
#[derive(Default)]
pub struct GenParams {
    pub seed: u32,
    /// Réglages du fBm érodé (cf. [`HeightParams`]).
    pub height: HeightParams,
}

/// **Le terrain procédural** : la graine et le générateur de relief, rien d'autre.
///
/// ## Données ≠ géométrie
/// Le terrain n'est **pas** stocké voxel par voxel : c'est un **champ de densité 3D**
/// (cf. [`DensityField`](super::super::generation::DensityField)), ré-échantillonné à
/// la volée par le mailleur. Il n'y a donc rien à « charger » pour un chunk : le seul
/// stockage du monde est l'overlay d'édits, qui vit ailleurs (cf. [`ChunkData`]).
///
/// ## Réellement immuable
/// Rien ici ne dépend ni de la caméra, ni du jeu : la graine ne change jamais. C'est ce
/// qui permet de le partager en `Arc` avec les threads de maillage sans copie ni verrou,
/// **sans réserve** — contrairement aux édits, qui bougent et voyagent donc en
/// instantané (cf. [`TerrainSnapshot`](super::chunk_store::TerrainSnapshot)). Le niveau
/// de détail, lui, vit dans la [`LodGrid`](super::super::lod::grid::LodGrid).
///
/// [`ChunkData`]: super::chunk_data::ChunkData
pub struct TerrainSource {
    /// Générateur du relief (fBm). Alimente le champ de densité.
    height: HeightField,
}

impl TerrainSource {
    pub fn new(params: GenParams) -> TerrainSource {
        TerrainSource {
            height: HeightField::new(params.seed, params.height),
        }
    }

    /// Le générateur de relief, pour le [`DensityField`](super::super::generation::DensityField).
    pub(super) fn height_field(&self) -> &HeightField {
        &self.height
    }

    /// Altitude du relief (en voxels) à la colonne monde `(wx, wy)`.
    pub fn terrain_height(&self, wx: f32, wy: f32) -> f32 {
        self.height.height(wx as f64, wy as f64) as f32
    }

    /// Altitude **moyenne** du relief sur `coords`, mesurée au centre des chunks.
    ///
    /// C'est le plan de référence par rapport auquel on juge « être haut » (cf.
    /// [`LodFocus`](super::super::lod::LodFocus)) : survoler à 400 unités au-dessus de la
    /// plaine moyenne doit dégrader le LOD, se tenir au fond d'une vallée non.
    ///
    /// Échantillonné 1 chunk sur 16 : le fBm érodé coûte ~3 évaluations de Perlin par
    /// octave, et une moyenne n'a pas besoin de plus de quelques centaines de points.
    pub fn mean_terrain_height(&self, coords: &[IVec2]) -> f32 {
        let half = CHUNK_SIZE as f32 / 2.0;
        let mut sum = 0.0;
        let mut n = 0;
        for coord in coords.iter().step_by(16) {
            let x = (coord.x * CHUNK_SIZE as i32) as f32 + half;
            let y = (coord.y * CHUNK_SIZE as i32) as f32 + half;
            sum += self.terrain_height(x, y);
            n += 1;
        }
        if n == 0 { 0.0 } else { sum / n as f32 }
    }
}
