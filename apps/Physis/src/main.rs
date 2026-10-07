use anyhow::Result;
use dromon_engine::config::WorldGenConfig;
use dromon_engine::{Scene, Timer, World};

struct Physis;

impl Scene for Physis {
    fn setup(&mut self, world: &mut World) -> Result<()> {
        // `config/world_macro.ron` + `config/world_detail.ron` à la racine du workspace.
        let config = WorldGenConfig::load()?;
        world.generate_terrain(&config)?;
        Ok(())
    }

    fn update(&mut self, _world: &mut World, _timer: &Timer) {}
}

fn main() -> Result<()> {
    dromon_engine::run(Physis)
}
