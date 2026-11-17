//! Mode lookdev (`debug.ron` → `lookdev`) : un sol plat à la place du terrain, pour
//! examiner des assets.

use anyhow::{Context, Result};
use glam::{Vec2, Vec3};
use std::sync::Arc;

use crate::app::engine::renderer::render_resources::{ObjectVertex, RenderObject, Transform};
use crate::app::engine::renderer::world::World;
use crate::config::LookdevParams;

/// Nom du mesh et de la texture du sol dans le `RenderResourceManager`.
const GROUND: &str = "lookdev_ground";
/// Côté d'une case du damier (unités monde) : sert d'échelle sur les captures.
const CELL_SIZE: f32 = 1.0;
/// La texture couvre 2 × 2 cases.
const TEXTURE_SIZE: u32 = 256;
/// Gris sRGB ; 128 ≈ albédo 20 %, proche du gris neutre de 18 %.
const LIGHT: u8 = 230;
const DARK: u8 = 96;

impl World {
    /// Pose le sol et place la caméra face à l'origine. À la place du terrain, dans
    /// `generate_terrain`.
    pub(crate) fn setup_lookdev(&mut self) -> Result<()> {
        self.add_ground()?;
        self.camera.position = Vec3::new(-10.0, -10.0, 6.0);
        self.camera.look_at(Vec3::new(0.0, 0.0, 1.0));
        self.logger.info("lookdev : sol plat à la place du terrain");
        Ok(())
    }

    /// Hot reload `Lookdev` : entrée, sortie ou nouveau sol. `self.config` porte déjà les
    /// nouveaux réglages, `old` les anciens. Ce qui a été créé reste à envoyer au GPU
    /// (`Renderer::initialize`). Lookdev actif = `lookdev_world_gen` présent.
    pub(crate) fn hot_reload_lookdev(&mut self, old: &LookdevParams) -> Result<()> {
        let new = self.config.debug.lookdev;
        match (self.lookdev_world_gen.is_some(), new.enabled) {
            // Le sol remplace le terrain : une scène sans terrain reste telle quelle.
            (false, true) if self.terrain.is_some() => self.enter_lookdev(),
            (true, false) => self.leave_lookdev(),
            (true, true) if new.ground_size != old.ground_size => {
                self.wait_idle()?;
                self.remove_ground();
                self.add_ground()
            }
            _ => Ok(()),
        }
    }

    fn enter_lookdev(&mut self) -> Result<()> {
        self.wait_idle()?;
        let terrain = self.terrain.take().context("terrain absent")?;
        self.lookdev_world_gen = Some(terrain.world_gen);
        self.water = None;
        self.world_maps = None;
        self.materials = None;
        self.light.shadow = self.config.render.shadow.static_scene.clone();
        self.bound_camera(None, self.config.debug.camera_unbounded);
        self.setup_lookdev()
    }

    /// Objets posés retirés (ils flotteraient dans le monde), caméra à sa pose de départ.
    fn leave_lookdev(&mut self) -> Result<()> {
        let Some(world_gen) = self.lookdev_world_gen.take() else {
            return Ok(());
        };
        self.wait_idle()?;
        self.lookdev_objects.clear();
        self.rrm.meshes.remove(GROUND);
        // Avant le terrain : son streaming part de la caméra.
        let params = &self.config.render.camera;
        self.camera
            .teleport(
                params.start_position,
                Some(params.start_yaw_deg.to_radians()),
                Some(params.start_pitch_deg.to_radians()),
            )
            .map_err(anyhow::Error::msg)?;
        self.generate_terrain(&world_gen)
    }

    /// Sol au `ground_size` courant. Texture créée une fois, gardée ensuite.
    fn add_ground(&mut self) -> Result<()> {
        let half = self.config.debug.lookdev.ground_size / 2.0;
        let (vertices, indices) = ground_quad(half);
        self.rrm.add_mesh_from_data(GROUND.to_owned(), vertices, indices)?;
        if self.rrm.get_texture(GROUND).is_none() {
            self.rrm
                .add_texture_from_data(GROUND.to_owned(), TEXTURE_SIZE, TEXTURE_SIZE, &checker())?;
        }
        self.lookdev_objects.push(RenderObject::new(
            self.rrm.get_mesh(GROUND).context("mesh du sol absent")?,
            self.rrm.get_texture(GROUND).context("texture du sol absente")?,
            Transform::default(),
        ));
        Ok(())
    }

    /// GPU au repos : les frames en vol dessinent peut-être ce qui va être détruit.
    fn remove_ground(&mut self) {
        if let Some(mesh) = self.rrm.meshes.remove(GROUND) {
            self.lookdev_objects.retain(|object| !Arc::ptr_eq(&object.mesh, &mesh));
        }
    }

    fn wait_idle(&self) -> Result<()> {
        unsafe { self.context.device.device_wait_idle()? };
        Ok(())
    }
}

/// Carré en z = 0, normale +Z, sens antihoraire vu du dessus. UV = position monde :
/// l'échelle du damier ne dépend pas de la taille du sol.
fn ground_quad(half: f32) -> (Vec<ObjectVertex>, Vec<u32>) {
    let corners = [(-half, -half), (half, -half), (half, half), (-half, half)];
    let vertices = corners
        .iter()
        .map(|&(x, y)| ObjectVertex {
            pos: Vec3::new(x, y, 0.0),
            color: Vec3::ONE,
            texCoord: Vec2::new(x, y) / (2.0 * CELL_SIZE),
            normal: Vec3::Z,
        })
        .collect();
    (vertices, vec![0, 1, 2, 2, 3, 0])
}

/// Damier de 2 × 2 cases, RGBA8 sRGB.
fn checker() -> Vec<u8> {
    let half = TEXTURE_SIZE / 2;
    let mut pixels = Vec::with_capacity((TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
    for y in 0..TEXTURE_SIZE {
        for x in 0..TEXTURE_SIZE {
            let v = if (x < half) == (y < half) {
                LIGHT
            } else {
                DARK
            };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    pixels
}
