use super::chunk::Chunk;
use super::density_field::DensityField;
use super::height_field::{HeightField, HeightParams};
use super::lod::{self, Face, TransitionFaces};
use crate::profile;
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
        self.chunks
            .entry(coord)
            .or_insert_with(|| Chunk::new(coord));
        &self.chunks[&coord]
    }

    /// Niveau de LOD du chunk `coord` (0 si inconnu). Lu par le mailleur pour choisir
    /// le pas d'échantillonnage. Source de vérité unique du LOD.
    pub fn chunk_lod(&self, coord: IVec2) -> u8 {
        self.chunks.get(&coord).map_or(0, |c| c.lod_level)
    }

    /// Fixe le niveau de LOD du chunk `coord` (no-op s'il n'est pas chargé). Plus tard,
    /// le LOD dynamique appellera ceci puis re-maillera le chunk concerné.
    pub fn set_chunk_lod(&mut self, coord: IVec2, lod: u8) {
        if let Some(c) = self.chunks.get_mut(&coord) {
            c.lod_level = lod;
        }
    }

    /// (Re)calcule le masque de faces de transition du chunk `coord` depuis le LOD de ses
    /// voisins et le **stocke** sur le chunk (cf. [`Chunk::transition_faces`]). À appeler
    /// après toute (ré)assignation de LOD, une fois TOUS les voisins fixés — le masque
    /// dépend d'eux. No-op si le chunk n'est pas chargé.
    pub fn refresh_transition_faces(&mut self, coord: IVec2) {
        let faces = self.compute_transition_faces(coord);
        if let Some(c) = self.chunks.get_mut(&coord) {
            c.transition_faces = faces;
        }
    }

    /// Masque de transition **stocké** du chunk `coord` (vide si non chargé). Lu par le
    /// mailleur pour savoir quels bords coudre ; valide tant que les LOD n'ont pas bougé
    /// depuis le dernier [`ChunkManager::refresh_transition_faces`].
    pub fn chunk_transition_faces(&self, coord: IVec2) -> TransitionFaces {
        self.chunks
            .get(&coord)
            .map_or(TransitionFaces::default(), |c| c.transition_faces)
    }

    /// Règle de détection appliquée au voisinage courant (dérivée, pure) : une face porte
    /// une transition ssi son voisin est plus grossier. LOD des 4 voisins rassemblé ici
    /// (0 si non chargé ⇒ jamais plus grossier). La source de vérité reste les LOD.
    fn compute_transition_faces(&self, coord: IVec2) -> TransitionFaces {
        let my_lod = self.chunk_lod(coord);
        let neighbor_lods = Face::ALL.map(|f| self.chunk_lod(coord + f.offset()));
        lod::transition_faces(my_lod, neighbor_lods)
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
