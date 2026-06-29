use anyhow::Result;
use dromon_engine::{GenParams, HeightParams, Scene, Timer, World};

struct Physis;

impl Scene for Physis {
    fn setup(&mut self, world: &mut World) -> Result<()> {
        // Génère une grille de 2×2 chunks de terrain autour de l'origine.
        world.generate_terrain(
            GenParams {
                seed: 10,
                height: HeightParams {
                    base_height: 16.0,
                    amplitude: 220.0, // base + amplitude = 220 < CHUNK_HEIGHT (256) :
                    // même la crête la plus vive reste sous le plafond du monde voxel
                    frequency: 0.0017,
                    octaves: 8,
                    lacunarity: 2.0,
                    gain: 0.5,
                    erosion: 0.6,           // creuse les vallées sans tout lisser
                    ridge: 0.9,             // arêtes vives → aspect montagne escarpée
                    lowland_flatness: 0.85, // plaines douces, détail réservé à l'altitude
                },
            },
            26,
            26,
        )?;
        Ok(())
    }

    fn update(&mut self, _world: &mut World, _timer: &Timer) {}
}

fn main() -> Result<()> {
    dromon_engine::run(Physis)
}
