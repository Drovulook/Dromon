use anyhow::Result;
use dromon_engine::config::WorldGenConfig;
use dromon_engine::{Scene, Timer, World};

/// Bac à sable de l'agent Claude (cf. README.md) : pour l'instant le monde de Physis,
/// avec sa propre copie de `config/` et `res/`.
struct AiSandbox;

impl Scene for AiSandbox {
    fn setup(&mut self, world: &mut World) -> Result<()> {
        let config = WorldGenConfig::load()?;
        world.generate_terrain(&config)?;
        Ok(())
    }

    fn update(&mut self, _world: &mut World, _timer: &Timer) {}
}

fn main() -> Result<()> {
    dromon_engine::run(AiSandbox, dromon_engine::app_dirs!())
}
