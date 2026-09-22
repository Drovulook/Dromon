use anyhow::Result;
use dromon_engine::{GenParams, HeightParams, Scene, Timer, World};

struct Physis;

impl Scene for Physis {
    fn setup(&mut self, world: &mut World) -> Result<()> {
        // Disque de terrain de 40 chunks de rayon autour de l'origine
        // (2560 unités monde, ~5000 chunks).
        world.generate_terrain(
            GenParams {
                seed: 20,
                height: HeightParams {
                    base_height: 26.0,
                    amplitude: 480.0, // base + amplitude = 506 < CHUNK_HEIGHT (512) :
                    // même la crête la plus vive reste sous le plafond du monde voxel
                    frequency: 0.00065,
                    octaves: 7,
                    lacunarity: 2.15, // !! doit être non entière pour éviter l'alignement des octaves
                    gain: 0.5,        // la renormalisation rend `ridge` neutre en amplitude
                    erosion: 0.4,     // creuse les vallées sans tout lisser
                    ridge: 0.80,      // arêtes vives → aspect montagne escarpée
                    lowland_flatness: 0.75, // plaines douces, détail réservé à l'altitude
                    ridge_altitude: 0.8, // arêtes vives en altitude, vallées arrondies
                },
            },
            80,
        )?;
        Ok(())
    }

    fn update(&mut self, _world: &mut World, _timer: &Timer) {}
}

fn main() -> Result<()> {
    dromon_engine::run(Physis)
}
