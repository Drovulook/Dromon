use anyhow::Result;
use dromon_engine::{GenParams, Scene, Timer, World};

struct Physis;

impl Scene for Physis {
    fn setup(&mut self, world: &mut World) -> Result<()> {
        // Génère une grille de 2×2 chunks de terrain autour de l'origine.
        world.generate_terrain(
            GenParams {
                seed: 1337,
                base_height: 6.0,
                amplitude: 20.0,
                frequency: 0.02,
                octaves: 4,
            },
            20,
            20,
        )?;
        Ok(())
    }

    fn update(&mut self, _world: &mut World, _timer: &Timer) {}
}

fn main() -> Result<()> {
    dromon_engine::run(Physis)
}
