//! **Stockage des données de chunk** et instantané confié aux threads de maillage.
//!
//! Le magasin est **épars** : il ne contient que les chunks *modifiés*, pas les chunks
//! *chargés*. Les deux ensembles ont des durées de vie différentes — un chunk qui sort
//! de la portée de la caméra jette son mesh mais garde ses édits, seule chose du monde
//! qui ne se recalcule pas.

use glam::{IVec2, IVec3};
use rustc_hash::FxHashMap;
use std::sync::Arc;

use super::{CHUNK_SIZE, chunk_data::ChunkData, terrain_source::TerrainSource};
use crate::app::engine::terrain_generation::generation::DensityField;

/// Chunk contenant la colonne monde `(wx, wy)`, et coordonnée locale associée.
#[inline]
fn split_world_coord(world: IVec3) -> (IVec2, IVec3) {
    let size = CHUNK_SIZE as i32;
    let coord = IVec2::new(world.x.div_euclid(size), world.y.div_euclid(size));
    let local = IVec3::new(world.x.rem_euclid(size), world.y.rem_euclid(size), world.z);
    (coord, local)
}

/// Les [`ChunkData`] du monde, indexés par coordonnée de chunk.
///
/// ## Copie à l'écriture
/// Chaque chunk est derrière son propre `Arc` : éditer clone **le seul chunk touché**,
/// et les lots de maillage déjà partis continuent de lire l'ancienne version sans
/// verrou. C'est le patron déjà employé pour la [`LodGrid`](super::super::lod::grid::LodGrid) —
/// un worker doit voir une configuration figée pendant tout son lot.
///
#[derive(Default)]
pub struct ChunkStore {
    chunks: FxHashMap<IVec2, Arc<ChunkData>>,
}

impl ChunkStore {
    /// Écrit une densité en coordonnées **monde**. Crée le `ChunkData` au besoin.
    pub fn edit(&mut self, world: IVec3, density: f32) {
        let (coord, local) = split_world_coord(world);
        let data = self.chunks.entry(coord).or_default();
        // `make_mut` clone si et seulement si un lot en vol détient encore l'ancienne
        // version — sinon il écrit en place.
        Arc::make_mut(data).set_edit(local, density);
    }

    /// Aucun chunk modifié : le monde est purement procédural.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

/// Vue **figée** du terrain confiée aux threads de maillage : le relief (immuable) et
/// les édits tels qu'ils étaient à la soumission du lot.
///
/// Le clone est bon marché — des `Arc`, et seulement pour les chunks édités.
#[derive(Clone)]
pub struct TerrainSnapshot {
    source: Arc<TerrainSource>,
    edits: FxHashMap<IVec2, Arc<ChunkData>>,
}

impl TerrainSnapshot {
    pub fn new(source: &Arc<TerrainSource>, store: &ChunkStore) -> TerrainSnapshot {
        TerrainSnapshot {
            source: source.clone(),
            edits: store.chunks.clone(),
        }
    }

    /// Construit le [`DensityField`] échantillonnable sur la région du chunk `coord`.
    /// C'est l'unique interface entre le terrain et le mailleur : celui-ci n'appelle
    /// que `sample`/`vertical_bounds`, sans rien savoir du relief ni des grottes.
    ///
    /// `apron` = marge (en voxels) autour du chunk que le mailleur échantillonnera
    /// au-delà de ses bords (le rayon du stencil des normales).
    pub fn density_field(&self, coord: IVec2, apron: i32) -> DensityField<'_> {
        DensityField::new(
            self.source.height_field(),
            self.region_edits(coord, apron),
            coord,
            apron,
        )
    }

    /// Édits couvrant la région échantillonnée (chunk + `apron`), **en coordonnées
    /// monde**, fusionnés depuis les chunks voisins.
    ///
    /// L'apron déborde sur les chunks adjacents : leurs édits comptent, sinon une
    /// tranchée creusée à cheval sur une frontière donnerait deux normales différentes
    /// de part et d'autre. On fusionne une fois par maillage plutôt que de tester le
    /// voisinage à chaque échantillon — la boucle chaude garde une table unique et son
    /// court-circuit.
    fn region_edits(&self, coord: IVec2, apron: i32) -> FxHashMap<IVec3, f32> {
        let mut merged = FxHashMap::default();
        if self.edits.is_empty() {
            return merged; // chemin normal : aucune allocation
        }

        let size = CHUNK_SIZE as i32;
        let origin = coord * size;
        let (lo, hi) = (origin - apron, origin + size + apron);
        let (c_lo, c_hi) = (
            IVec2::new(lo.x.div_euclid(size), lo.y.div_euclid(size)),
            IVec2::new(hi.x.div_euclid(size), hi.y.div_euclid(size)),
        );

        for cx in c_lo.x..=c_hi.x {
            for cy in c_lo.y..=c_hi.y {
                let neighbor = IVec2::new(cx, cy);
                let Some(data) = self.edits.get(&neighbor) else {
                    continue;
                };
                let base = neighbor * size;
                for (local, density) in data.edits() {
                    let world = IVec3::new(base.x + local.x, base.y + local.y, local.z);
                    if (lo.x..=hi.x).contains(&world.x) && (lo.y..=hi.y).contains(&world.y) {
                        merged.insert(world, density);
                    }
                }
            }
        }
        merged
    }
}
