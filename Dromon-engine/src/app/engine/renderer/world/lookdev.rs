//! Mode lookdev (`debug.ron` → `lookdev`) : un sol plat à la place du terrain, pour
//! examiner des assets.

use anyhow::{Context, Result, bail};
use glam::{Vec2, Vec3};

use crate::app::engine::renderer::render_resources::{
    MeshLoadOptions, ObjectVertex, RenderObject, Transform,
};
use crate::app::engine::renderer::world::World;
use crate::config::{LookdevParams, ModelSource};

/// Nom du mesh et de la texture du sol dans le `RenderResourceManager`.
const GROUND: &str = "lookdev_ground";
/// Id du sol, que `remove` refuse ; les objets posés partent de `FIRST_OBJECT_ID`.
const GROUND_ID: u32 = 0;
pub(crate) const FIRST_OBJECT_ID: u32 = 1;
/// Préfixe des meshes des assets posés dans le `RenderResourceManager` (cache vidé par
/// `clear`).
const MODEL_PREFIX: &str = "lookdev_model/";
/// Texture des assets en attendant leurs matériaux : gris uni, sRGB 188 ≈ albédo 50 %.
const DEFAULT_TEXTURE: &str = "lookdev_default";
const DEFAULT_GREY: u8 = 188;
/// Côté d'une case du damier (unités monde) : sert d'échelle sur les captures.
const CELL_SIZE: f32 = 1.0;
/// La texture couvre 2 × 2 cases.
const TEXTURE_SIZE: u32 = 256;
/// Gris sRGB ; 230 ≈ albédo 79 %, 96 ≈ 12 %.
const LIGHT: u8 = 230;
const DARK: u8 = 96;

/// Objet du lookdev : le sol ou un asset posé par commande.
pub(crate) struct LookdevObject {
    /// Stable : un index changerait quand on retire un objet placé avant.
    pub id: u32,
    pub object: RenderObject,
}

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
        self.rrm.meshes.retain(|name, _| !name.starts_with(MODEL_PREFIX));
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
        self.lookdev_objects.push(LookdevObject {
            id: GROUND_ID,
            object: RenderObject::new(
                self.rrm.get_mesh(GROUND).context("mesh du sol absent")?,
                self.rrm.get_texture(GROUND).context("texture du sol absente")?,
                Transform::default(),
            ),
        });
        Ok(())
    }

    /// GPU au repos : les frames en vol dessinent peut-être ce qui va être détruit.
    fn remove_ground(&mut self) {
        self.lookdev_objects.retain(|o| o.id != GROUND_ID);
        self.rrm.meshes.remove(GROUND);
    }

    /// Pose un asset et renvoie son id. Le fichier n'est relu que si le mesh n'est pas en
    /// cache (cf. `clear_lookdev_objects`). Bloquant : envoi immédiat au GPU.
    pub(crate) fn spawn_lookdev_object(
        &mut self,
        source: &ModelSource,
        transform: Transform,
    ) -> Result<u32> {
        if self.lookdev_world_gen.is_none() {
            bail!("lookdev inactif (debug.ron → lookdev.enabled)");
        }
        let name = format!("{MODEL_PREFIX}{source:?}");
        if self.rrm.get_mesh(&name).is_none() {
            // Assets : unités, origine et normales du fichier.
            let options = MeshLoadOptions { smooth: false, normalize: false };
            self.rrm.add_mesh(name.clone(), source, options)?;
        }
        if self.rrm.get_texture(DEFAULT_TEXTURE).is_none() {
            let texel = [DEFAULT_GREY, DEFAULT_GREY, DEFAULT_GREY, 255];
            self.rrm.add_texture_from_data(DEFAULT_TEXTURE.to_owned(), 1, 1, &texel)?;
        }
        self.rrm.upload_pending()?;

        let id = self.lookdev_next_id;
        self.lookdev_next_id += 1;
        self.lookdev_objects.push(LookdevObject {
            id,
            object: RenderObject::new(
                self.rrm.get_mesh(&name).context("mesh de l'asset absent")?,
                self.rrm.get_texture(DEFAULT_TEXTURE).context("texture par défaut absente")?,
                transform,
            ),
        });
        Ok(id)
    }

    /// Le mesh reste en cache : aucun buffer détruit, pas d'attente du GPU.
    pub(crate) fn remove_lookdev_object(&mut self, id: u32) -> Result<()> {
        if id == GROUND_ID {
            bail!("le sol (id {GROUND_ID}) ne se retire pas");
        }
        let index = self
            .lookdev_objects
            .iter()
            .position(|o| o.id == id)
            .with_context(|| format!("aucun objet d'id {id}"))?;
        self.lookdev_objects.remove(index);
        Ok(())
    }

    /// Retire tous les objets posés (pas le sol) et vide le cache des modèles : un modèle
    /// régénéré sera relu au prochain `spawn`. Renvoie le nombre d'objets retirés.
    pub(crate) fn clear_lookdev_objects(&mut self) -> Result<usize> {
        let before = self.lookdev_objects.len();
        self.lookdev_objects.retain(|o| o.id == GROUND_ID);
        // Le cache tient le dernier `Arc` : buffers détruits en le vidant, donc après
        // les frames en vol.
        self.wait_idle()?;
        self.rrm.meshes.retain(|name, _| !name.starts_with(MODEL_PREFIX));
        Ok(before - self.lookdev_objects.len())
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
