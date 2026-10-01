use crate::{HeightParams, app::engine::terrain_generation::generation::height_field::HeightField};
use crate::app::engine::terrain_generation::lod::chunk_distance;
use glam::{IVec2, Vec2};

use super::CHUNK_SIZE;

/// Paramètres de génération du monde : graine, forme du champ d'altitude, étendue.
#[derive(Default)]
pub struct GenParams {
    pub seed: u32,
    /// Réglages du fBm érodé (cf. [`HeightParams`]).
    pub height: HeightParams,
    /// Rayon du monde, en chunks (cf. [`WorldDisc`]). Sans rapport avec la portée de
    /// chargement, qui se règle à part.
    pub world_radius: u32,
}

/// **Forme du monde** : un disque de chunks centré sur l'origine. Seul critère de bord du
/// monde pour le mailleur (murs de bordure), indépendant de ce qui est chargé.
///
/// Disque plutôt que carré : la distance au bord du monde ne dépend plus de la
/// direction. À nombre de chunks égal, un carré ne garantit que `0,89 · r` dans les
/// directions des axes, et en offre `1,25 · r` dans les diagonales — dépensés là où
/// le joueur ne va pas plus souvent qu'ailleurs.
#[derive(Clone, Copy)]
pub struct WorldDisc {
    /// Rayon en unités monde.
    radius: f32,
}

impl WorldDisc {
    pub fn new(radius_chunks: u32) -> WorldDisc {
        WorldDisc {
            radius: (radius_chunks as usize * CHUNK_SIZE) as f32,
        }
    }

    /// Le chunk `coord` fait-il partie du monde ? Son CENTRE doit tomber dans le disque —
    /// même mesure que la politique de LOD, dont les anneaux sont donc concentriques au
    /// bord du monde.
    #[inline]
    pub fn contains(self, coord: IVec2) -> bool {
        chunk_distance(coord, Vec2::ZERO) <= self.radius
    }

    /// Tous les chunks du monde. Les centres valant `c·64 + 32`, ils sont symétriques
    /// autour de 0 et la bordure `c = ±r` du carré de balayage est toujours rejetée.
    pub fn coords(self) -> Vec<IVec2> {
        let r = (self.radius / CHUNK_SIZE as f32) as i32;
        let mut coords = Vec::new();
        for cx in -r..=r {
            for cy in -r..=r {
                let coord = IVec2::new(cx, cy);
                if self.contains(coord) {
                    coords.push(coord);
                }
            }
        }
        coords
    }
}

/// **Le terrain procédural** : la graine, le générateur de relief et la forme du monde,
/// rien d'autre.
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
    world: WorldDisc,
}

impl TerrainSource {
    pub fn new(params: GenParams) -> TerrainSource {
        TerrainSource {
            height: HeightField::new(params.seed, params.height),
            world: WorldDisc::new(params.world_radius),
        }
    }

    /// Le générateur de relief, pour le [`DensityField`](super::super::generation::DensityField).
    pub(super) fn height_field(&self) -> &HeightField {
        &self.height
    }

    /// La forme du monde.
    pub fn world(&self) -> WorldDisc {
        self.world
    }

    /// Altitude du relief (en voxels) à la colonne monde `(wx, wy)`.
    pub fn terrain_height(&self, wx: f32, wy: f32) -> f32 {
        self.height.height(wx as f64, wy as f64) as f32
    }

    /// Altitude **moyenne** du relief sur tout le monde, mesurée au centre des chunks.
    ///
    /// C'est le plan de référence par rapport auquel on juge « être haut » (cf.
    /// [`LodFocus`](super::super::lod::LodFocus)) : survoler à 400 unités au-dessus de la
    /// plaine moyenne doit dégrader le LOD, se tenir au fond d'une vallée non.
    ///
    /// ~500 échantillons quelle que soit la taille du monde : le fBm érodé coûte ~3
    /// évaluations de Perlin par octave, et une moyenne n'a pas besoin de plus.
    pub fn mean_terrain_height(&self) -> f32 {
        let coords = self.world.coords();
        let stride = (coords.len() / 512).max(1);
        let half = CHUNK_SIZE as f32 / 2.0;
        let mut sum = 0.0;
        let mut n = 0;
        for coord in coords.iter().step_by(stride) {
            let x = (coord.x * CHUNK_SIZE as i32) as f32 + half;
            let y = (coord.y * CHUNK_SIZE as i32) as f32 + half;
            sum += self.terrain_height(x, y);
            n += 1;
        }
        if n == 0 { 0.0 } else { sum / n as f32 }
    }
}
