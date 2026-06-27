use anyhow::{Context, Result};
use dromon_engine::{RenderObject, Scene, Timer, Transform, World};

// Chemins relatifs au crate du MOTEUR : ils sont résolus via le `CARGO_MANIFEST_DIR`
// de `Dromon-engine` (cf. mesh_handler/texture_handler), donc les assets restent
// dans `Dromon-engine/res/` même si on lance depuis ce binaire.
const GLTF_PATH: &str = "/res/models/dromon_ship/scene.gltf";
const TEXTURE_PATH: &str = "/res/models/dromon_ship/DefaultMaterial_baseColor.png";

/// Bac à sable de visualisation de modèles. Implémente `Scene` : le moteur appelle
/// `setup` une fois (chargement + placement des objets) puis `update` à chaque frame.
struct ModelSandbox;

impl Scene for ModelSandbox {
    fn setup(&mut self, world: &mut World) -> Result<()> {
        let rorm = &mut world.rorm;
        rorm.add_texture("texture1".to_string(), TEXTURE_PATH.to_string())?;
        rorm.add_texture("texture2".to_string(), TEXTURE_PATH.to_string())?;
        rorm.add_mesh("mesh1".to_string(), GLTF_PATH.to_string(), true)?;
        rorm.add_mesh("mesh2".to_string(), GLTF_PATH.to_string(), true)?;

        let render_objects = vec![
            RenderObject::new(
                rorm.get_mesh("mesh1")
                    .context("mesh \"mesh1\" introuvable")?,
                rorm.get_texture("texture1")
                    .context("texture \"texture1\" introuvable")?,
                Transform {
                    translation: glam::Vec3::new(0.0, 1.0, 0.0),
                    ..Default::default()
                },
            ),
            RenderObject::new(
                rorm.get_mesh("mesh2")
                    .context("mesh \"mesh2\" introuvable")?,
                rorm.get_texture("texture2")
                    .context("texture \"texture2\" introuvable")?,
                Transform {
                    translation: glam::Vec3::new(0.0, -1.0, 0.0),
                    ..Default::default()
                },
            ),
        ];

        world.render_objects = render_objects;
        Ok(())
    }

    fn update(&mut self, world: &mut World, timer: &Timer) {
        for render_object in &mut world.render_objects {
            render_object.transform.rotation.z += timer.delta_secs();
        }
    }
}

fn main() -> Result<()> {
    dromon_engine::run(ModelSandbox)
}
