use crate::{
    HeightParams,
    app::engine::terrain_generation::{
        chunk::{CHUNK_SIZE, Chunk},
        generation::{DensityField, height_field::HeightField},
    },
    profile,
};
use glam::{IVec2, IVec3};
use std::collections::HashMap;

/// Paramètres de génération du relief : graine + forme du champ d'altitude.
pub struct GenParams {
    pub seed: u32,
    /// Réglages du fBm érodé (cf. [`HeightParams`]).
    pub height: HeightParams,
}

impl Default for GenParams {
    fn default() -> Self {
        GenParams {
            seed: 0,
            height: HeightParams::default(),
        }
    }
}

/// Recense les chunks chargés, détient le générateur de relief et l'overlay d'édits.
/// Point d'entrée du terrain procédural.
///
/// ## Données ≠ géométrie
/// Le terrain n'est **pas** stocké voxel par voxel : c'est un **champ de densité 3D**
/// (cf. [`DensityField`]), ré-échantillonné à la volée par le mailleur. Les seules
/// données réellement stockées sont les **édits** du joueur (creuser/remblayer) :
/// on ne paie que ce qui est modifié.
///
/// ## Immuable une fois généré
/// Rien ici ne dépend de la caméra : le relief ne change pas quand on se déplace. C'est
/// ce qui permet de le partager en `Arc` avec les threads de maillage sans copie ni
/// verrou. Le niveau de détail, lui, vit dans la
/// [`LodGrid`](super::super::lod::grid::LodGrid).
pub struct ChunkManager {
    /// Régions chargées (simples marqueurs, cf. [`Chunk`]).
    chunks: HashMap<IVec2, Chunk>,
    /// Générateur du relief (fBm). Alimente le champ de densité.
    height: HeightField,
    /// Overlay épars des densités **modifiées**, en coordonnées voxel monde.
    /// Prioritaire sur la densité procédurale. Vide tant qu'on n'édite pas.
    edits: HashMap<IVec3, f32>,
}

impl ChunkManager {
    pub fn new(params: GenParams) -> ChunkManager {
        let height = HeightField::new(params.seed, params.height);
        ChunkManager {
            chunks: HashMap::new(),
            height,
            edits: HashMap::new(),
        }
    }

    /// Enregistre la région `coord` comme chargée. Aucun calcul lourd : le terrain
    /// est produit à la demande au maillage (cf. [`ChunkManager::density_field`]).
    pub fn generate_chunk(&mut self, coord: IVec2) -> &Chunk {
        profile!();
        self.chunks.entry(coord).or_insert_with(|| Chunk::new(coord));
        &self.chunks[&coord]
    }

    /// Le chunk `coord` est-il enregistré ? ⚠ Le mailleur, lui, lit le bord du monde sur
    /// la `LodGrid` : c'est elle qui décrit la topologie du monde *affiché*.
    pub fn is_loaded(&self, coord: IVec2) -> bool {
        self.chunks.contains_key(&coord)
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

    /// Construit le [`DensityField`] échantillonnable sur la région du chunk `coord`.
    /// C'est l'unique interface entre le terrain et le mailleur : celui-ci n'appelle
    /// que `sample`/`vertical_bounds`, sans rien savoir du relief ni des grottes.
    ///
    /// `apron` = marge (en voxels) autour du chunk que le mailleur échantillonnera
    /// au-delà de ses bords (le rayon du stencil des normales).
    pub fn density_field(&self, coord: IVec2, apron: i32) -> DensityField<'_> {
        DensityField::new(&self.height, &self.edits, coord, apron)
    }
}
