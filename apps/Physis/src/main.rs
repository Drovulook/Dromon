use anyhow::{Context, Result};
use dromon_engine::{RenderObject, Scene, Timer, Transform, World};

struct Physis;

impl Scene for Physis {
    fn setup(&mut self, world: &mut World) -> Result<()> {
        let rorm = &mut world.rorm;

        let render_objects = vec![];

        world.render_objects = render_objects;
        Ok(())
    }

    fn update(&mut self, world: &mut World, timer: &Timer) {}
}

fn main() -> Result<()> {
    dromon_engine::run(Physis)
}
