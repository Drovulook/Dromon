use anyhow::Result;
use dromon_engine::{GenParams, HeightParams, Scene, Timer, World};

struct Physis;

impl Scene for Physis {
    fn setup(&mut self, world: &mut World) -> Result<()> {
        // Monde : disque de 200 chunks de rayon (12 800 unités, ~125 000 chunks).
        // Chargés : 80 chunks de rayon autour de la caméra (~20 000 chunks).
        world.generate_terrain(
            GenParams {
                seed: 20,
                world_radius: 400,
                height: HeightParams {
                    base_height: 50.0,
                    amplitude: 1200.0, // base + amplitude < CHUNK_HEIGHT (1024) :
                    // même la crête la plus vive reste sous le plafond du monde voxel
                    frequency: 0.00025,
                    octaves: 9,
                    lacunarity: 2.15, // !! doit être non entière pour éviter l'alignement des octaves
                    gain: 0.5,        // la renormalisation rend `ridge` neutre en amplitude
                    erosion: 0.085,   // creuse les vallées sans tout lisser
                    ridge: 0.80,      // arêtes vives → aspect montagne escarpée
                    lowland_flatness: 0.9, // plaines douces, détail réservé à l'altitude
                    ridge_altitude: 0.8, // arêtes vives en altitude, vallées arrondies
                    ridge_smoothness: 0.02, // arrondit les arêtes des grandes octaves
                    // Carte de massifs : longueur d'onde ~6700 < diamètre du monde (~25 600).
                    massif_frequency: 0.00010,
                    massif_low: -0.95,
                    massif_high: -0.55,
                    massif_min: 0.45, // 1.0 = désactivé
                    // Domain warping : déformations à l'échelle des montagnes.
                    warp_frequency: 0.00025,
                    warp_amplitude: 500.0, // 0.0 = désactivé
                },
            },
            80, // portée de chargement, en chunks
        )?;
        Ok(())
    }

    fn update(&mut self, _world: &mut World, _timer: &Timer) {}
}

fn main() -> Result<()> {
    dromon_engine::run(Physis)
}
