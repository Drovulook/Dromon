//! `render.ron` : qualité et réglages du rendu.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use super::ConfigFile;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RenderConfig {
    pub present_mode: PresentMode,
    pub camera: CameraParams,
    pub shadow: ShadowSettings,
    pub terrain: TerrainRenderParams,
}

impl ConfigFile for RenderConfig {
    const FILE_NAME: &'static str = "render.ron";

    fn validate(&self) -> Result<()> {
        let c = &self.camera;
        ensure!(
            0.0 < c.near && c.near < c.far,
            "camera : il faut 0 < near < far"
        );
        ensure!(
            0.0 < c.min_fov_deg && c.min_fov_deg <= c.fov_deg && c.fov_deg <= c.max_fov_deg
                && c.max_fov_deg < 180.0,
            "camera : il faut 0 < min_fov_deg ≤ fov_deg ≤ max_fov_deg < 180"
        );

        let s = &self.shadow;
        ensure!(s.map_resolution > 0, "shadow.map_resolution doit être > 0");
        for (name, b) in [("static_scene", &s.static_scene), ("terrain", &s.terrain)] {
            ensure!(b.half_size > 0.0, "shadow.{name}.half_size doit être > 0");
            ensure!(
                0.0 < b.near && b.near < b.far,
                "shadow.{name} : il faut 0 < near < far"
            );
        }

        let t = &self.terrain;
        ensure!(t.load_radius_chunks > 0, "terrain.load_radius_chunks doit être > 0");
        ensure!(t.unload_margin_chunks >= 0.0, "terrain.unload_margin_chunks doit être ≥ 0");
        ensure!(
            t.frame_budget_ms > 0.0 && t.initial_load_budget_ms > 0.0,
            "terrain : les budgets de temps doivent être > 0"
        );
        let lod = &t.lod;
        ensure!(
            lod.radii.len() <= MAX_LOD_LEVELS,
            "terrain.lod.radii : au plus {MAX_LOD_LEVELS} rayons (pas de 1 << lod ≤ taille d'un chunk)"
        );
        ensure!(
            lod.radii.first().is_none_or(|&r| r > 0.0)
                && lod.radii.windows(2).all(|r| r[0] < r[1]),
            "terrain.lod.radii doivent être > 0 et strictement croissants"
        );
        ensure!(
            (0.0..1.0).contains(&lod.hysteresis),
            "terrain.lod.hysteresis doit être dans [0, 1["
        );
        ensure!(lod.height_weight >= 0.0, "terrain.lod.height_weight doit être ≥ 0");
        Ok(())
    }
}

/// Nombre maximal de rayons de LOD : le pas du LOD `k` vaut `1 << k` voxels et ne peut
/// dépasser la taille d'un chunk (64 = `1 << 6`).
const MAX_LOD_LEVELS: usize = 6;

/// Terrain : portée de chargement, niveaux de détail, budgets.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerrainRenderParams {
    /// Portée de chargement autour de la caméra, en chunks. Sans rapport avec la taille
    /// du monde.
    pub load_radius_chunks: u32,
    /// **Hystérésis du chargement**, en chunks : un chunk entre dans la fenêtre sous la
    /// portée, mais n'en sort qu'au-delà de portée + marge. Sans elle, une caméra qui
    /// oscille sur le bord chargerait et déchargerait la même rangée en boucle.
    pub unload_margin_chunks: f32,
    pub lod: LodParams,
    /// Temps autorisé **par frame** pour bâtir les buffers d'un lot, et autant pour
    /// détruire les anciens (allocations Vulkan : ~100 µs–1 ms chacune). En temps plutôt
    /// qu'en nombre de meshes : le coût varie selon le driver. Au moins un mesh est
    /// toujours traité.
    pub frame_budget_ms: f32,
    /// Même budget pendant le **chargement initial** : des milliers de meshes à bâtir,
    /// et une fluidité moins critique qu'en jeu.
    pub initial_load_budget_ms: f32,
    /// Budget mémoire du cache des géométries de chunks, en Mo (allers-retours de
    /// caméra). En octets et non en entrées : LOD1 ≈ ¼ du LOD0, LOD2 ≈ 6 %…
    pub mesh_cache_mb: usize,
}

impl Default for TerrainRenderParams {
    fn default() -> Self {
        TerrainRenderParams {
            load_radius_chunks: 16,
            unload_margin_chunks: 2.0,
            lod: LodParams::default(),
            frame_budget_ms: 1.0,
            initial_load_budget_ms: 1.0,
            mesh_cache_mb: 2000,
        }
    }
}

/// Politique de LOD : distance œil → chunk ⇒ niveau de détail.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LodParams {
    /// Rayons (unités monde) des anneaux. `radii[k]` est la distance à partir de
    /// laquelle on passe du niveau `k` au niveau `k + 1` : leur nombre fixe le LOD max.
    pub radii: Vec<f32>,
    /// Demi-largeur relative de la **bande morte** de l'hystérésis (thermostat) : un
    /// chunk pile sur une frontière d'anneau oscillerait sinon entre deux niveaux.
    /// 0.08 ⇒ le rayon 800 se dédouble en 736 / 864.
    pub hysteresis: f32,
    /// Poids de la hauteur de caméra dans la distance qui pilote le LOD. `1.0` =
    /// distance 3D honnête ; plus bas atténue l'effet de l'altitude.
    pub height_weight: f32,
}

impl Default for LodParams {
    fn default() -> Self {
        LodParams {
            radii: vec![800.0, 1600.0, 3200.0, 6400.0],
            hysteresis: 0.08,
            height_weight: 1.0,
        }
    }
}

impl LodParams {
    /// Niveau de détail maximal : un par rayon franchi.
    pub fn max_lod(&self) -> u8 {
        self.radii.len() as u8
    }
}

/// Mode de présentation de la swapchain (synchronisation verticale).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresentMode {
    /// Vsync stricte : file d'attente, jamais de tearing. Seul mode garanti par Vulkan.
    Fifo,
    /// Comme `Fifo`, mais une image en retard est affichée tout de suite (tearing
    /// possible) au lieu d'attendre le rafraîchissement suivant.
    FifoRelaxed,
    /// Vsync sans file : l'image la plus récente remplace celle en attente. Pas de
    /// tearing, latence faible.
    #[default]
    Mailbox,
    /// Sans vsync : FPS illimités, tearing. Utile pour mesurer les performances.
    Immediate,
}

/// Caméra libre : projection et contrôles.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraParams {
    /// Champ de vision vertical au démarrage, en degrés.
    pub fov_deg: f32,
    /// Bornes du zoom (molette), en degrés.
    pub min_fov_deg: f32,
    pub max_fov_deg: f32,
    /// Plans de coupe. `far` borne la distance d'affichage : inutile au-delà de la
    /// portée de chargement du terrain.
    pub near: f32,
    pub far: f32,
    /// Vitesse de déplacement, en unités monde par seconde.
    pub move_speed: f32,
    /// Radians de rotation par pixel de souris.
    pub rotation_sensitivity: f32,
    /// Radians de FOV par cran de molette.
    pub zoom_speed: f32,
    /// Multiplicateurs appliqués tant que Alt est maintenue (rotation et zoom, puis
    /// déplacement).
    pub boost_factor_rot: f32,
    pub boost_factor_move: f32,
}

impl Default for CameraParams {
    fn default() -> Self {
        CameraParams {
            fov_deg: 45.0,
            min_fov_deg: 20.0,
            max_fov_deg: 110.0,
            near: 0.1,
            far: 8000.0,
            move_speed: 400.0,
            rotation_sensitivity: 0.002,
            zoom_speed: 0.05,
            boost_factor_rot: 4.0,
            boost_factor_move: 3.8,
        }
    }
}

/// Ombres : résolution de la shadow map et boîte de la lumière selon le type de scène.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShadowSettings {
    /// Côté de la shadow map (carrée), en texels. Plus grand = ombres plus fines, mais
    /// plus de mémoire et de remplissage.
    pub map_resolution: u32,
    /// Boîte d'une petite scène d'objets fixes (défaut de toute scène).
    pub static_scene: ShadowConfig,
    /// Boîte appliquée par `generate_terrain` : grande, et qui suit la caméra.
    pub terrain: ShadowConfig,
}

impl Default for ShadowSettings {
    fn default() -> Self {
        ShadowSettings {
            map_resolution: 2048,
            static_scene: ShadowConfig::default(),
            terrain: ShadowConfig {
                half_size: 2000.0,
                near: 1.0,
                far: 8000.0,
                eye_distance: 3000.0,
                follow_camera: true,
                focus_distance: 1500.0,
            },
        }
    }
}

/// « Frustum » orthographique de la shadow map. Étalée sur `2 * half_size` unités, la
/// shadow map donne `2 * half_size / map_resolution` u/texel : plus la boîte est
/// grande, plus on couvre de monde mais plus les ombres deviennent grossières.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShadowConfig {
    /// Demi-largeur/hauteur de la boîte orthographique, en unités monde.
    pub half_size: f32,
    /// Plans near/far le long de l'axe lumière, depuis l'« œil » virtuel.
    /// ⚠ `far` élargit aussi le biais anti-acné (exprimé en profondeur normalisée) :
    /// un `far` énorme sur de petits objets décolle leur ombre.
    pub near: f32,
    pub far: f32,
    /// Recul de l'« œil » virtuel le long de `-direction`.
    pub eye_distance: f32,
    /// Si `true`, la boîte suit la caméra (indispensable pour un grand terrain) ; sinon
    /// elle reste centrée sur l'origine (scène d'objets fixes).
    pub follow_camera: bool,
    /// Quand `follow_camera`, distance devant la caméra (le long du regard) du point
    /// de centrage : on dépense le budget d'ombre là où le joueur regarde.
    pub focus_distance: f32,
}

impl Default for ShadowConfig {
    /// Petite scène statique centrée sur l'origine.
    fn default() -> Self {
        ShadowConfig {
            half_size: 15.0,
            near: 0.1,
            far: 60.0,
            eye_distance: 30.0,
            follow_camera: false,
            focus_distance: 0.0,
        }
    }
}

impl ShadowConfig {
    /// Épaisseur de la boîte le long des rayons : convertit un biais en unités monde
    /// vers la profondeur normalisée de la shadow map.
    pub fn depth_range(&self) -> f32 {
        self.far - self.near
    }

    /// Côté d'un texel de la shadow map, en unités monde : l'échelle naturelle du biais
    /// anti-acné (l'erreur de profondeur grandit avec le texel).
    pub fn texel_size(&self, map_resolution: u32) -> f32 {
        2.0 * self.half_size / map_resolution as f32
    }
}
