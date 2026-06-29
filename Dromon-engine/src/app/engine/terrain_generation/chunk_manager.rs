pub use super::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, Chunk, Voxel};

use glam::IVec2;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use std::collections::HashMap;

// Matériaux de base (placeholders). À terme, remplacés par une vraie table de
// matériaux (roches, minerais, terre, etc.).
pub const MATERIAL_AIR: u16 = 0; // ID de matériau réservé à l'air (absence de matière).
pub const MATERIAL_ROCK: u16 = 1;
pub const MATERIAL_DIRT: u16 = 2;
pub const MATERIAL_GRASS: u16 = 3;
pub const MATERIAL_SNOW: u16 = 4;

/// Paramètres de génération du relief.
pub struct GenParams {
    pub seed: u32,
    /// Hauteur de base du terrain, en voxels (autour de laquelle le bruit oscille).
    pub base_height: f64,
    /// Amplitude verticale du relief, en voxels.
    pub amplitude: f64,
    /// Échelle horizontale du bruit : plus petit = collines plus larges.
    pub frequency: f64,
    /// Nombre d'octaves de fBm (couches de détail superposées).
    pub octaves: usize,
}

impl Default for GenParams {
    fn default() -> Self {
        GenParams {
            seed: 0,
            base_height: 64.0,
            amplitude: 24.0,
            frequency: 0.01,
            octaves: 5,
        }
    }
}

/// Détient les chunks générés et la source de bruit. Point d'entrée de la
/// génération procédurale du terrain.
pub struct ChunkManager {
    chunks: HashMap<IVec2, Chunk>,
    noise: Fbm<Perlin>,
    params: GenParams,
}

impl ChunkManager {
    pub fn new(params: GenParams) -> ChunkManager {
        // fBm (fractal Brownian motion) : empile plusieurs octaves de Perlin à
        // fréquences croissantes et amplitudes décroissantes, pour un relief
        // détaillé à plusieurs échelles (grandes collines + petites bosses).
        let noise = Fbm::<Perlin>::new(params.seed).set_octaves(params.octaves);
        ChunkManager {
            chunks: HashMap::new(),
            noise,
            params,
        }
    }

    /// Génère (ou régénère) le chunk à la coordonnée donnée, le stocke et en
    /// renvoie une référence.
    ///
    /// Approche heightmap pour ce premier jet : pour chaque colonne `(x, y)`, le
    /// bruit donne une hauteur de surface ; tout ce qui est en dessous est
    /// rempli de matière (roche, puis terre, puis herbe au sommet), tout ce qui
    /// est au-dessus reste de l'air.
    pub fn generate_chunk(&mut self, coord: IVec2) -> &Chunk {
        let mut chunk = Chunk::new(coord);

        // Origine du chunk en coordonnées voxel « monde ».
        let origin_x = coord.x * CHUNK_SIZE as i32;
        let origin_y = coord.y * CHUNK_SIZE as i32;

        for lx in 0..CHUNK_SIZE {
            for ly in 0..CHUNK_SIZE {
                let wx = (origin_x + lx as i32) as f64 * self.params.frequency;
                let wy = (origin_y + ly as i32) as f64 * self.params.frequency;

                // fBm renvoie ~[-1, 1] ; on le mappe vers une hauteur en voxels.
                let n = self.noise.get([wx, wy]);
                let height = (self.params.base_height + n * self.params.amplitude)
                    .round()
                    .clamp(0.0, (CHUNK_HEIGHT - 1) as f64) as usize;

                for z in 0..=height {
                    let material = if z == height {
                        MATERIAL_GRASS
                    } else if z + 4 >= height {
                        MATERIAL_DIRT
                    } else {
                        MATERIAL_ROCK
                    };
                    chunk.set_voxel(lx, ly, z, Voxel::solid(material));
                }
            }
        }

        self.chunks.insert(coord, chunk);
        &self.chunks[&coord]
    }

    /// Chunk déjà généré à cette coordonnée, le cas échéant.
    pub fn get_chunk(&self, coord: IVec2) -> Option<&Chunk> {
        self.chunks.get(&coord)
    }

    /// Surface à une position **monde** `(world_x, world_y)` : `z` du voxel
    /// non-air le plus haut + poids de matériaux normalisés en `[0, 1]`.
    ///
    /// Renvoie `None` si aucun chunk n'a été généré à cet endroit (ou colonne
    /// vide). Travaille en coordonnées monde pour que le mesher puisse aussi
    /// interroger les chunks **voisins** au niveau des bords — c'est ce qui
    /// rend la couture entre chunks continue.
    pub fn surface_at(&self, world_x: i32, world_y: i32) -> Option<(i32, [f32; 4])> {
        let size = CHUNK_SIZE as i32;
        // div_euclid / rem_euclid : division « plancher » correcte pour les
        // coordonnées négatives (contrairement à `/` et `%` qui tronquent).
        let coord = IVec2::new(world_x.div_euclid(size), world_y.div_euclid(size));
        let chunk = self.chunks.get(&coord)?;
        let lx = world_x.rem_euclid(size) as usize;
        let ly = world_y.rem_euclid(size) as usize;
        let z = chunk.surface_height(lx, ly)?;
        let v = chunk.get_voxel(lx, ly, z);
        let weights = [
            v.weights[0] as f32 / 255.0,
            v.weights[1] as f32 / 255.0,
            v.weights[2] as f32 / 255.0,
            v.weights[3] as f32 / 255.0,
        ];
        Some((z as i32, weights))
    }
}
