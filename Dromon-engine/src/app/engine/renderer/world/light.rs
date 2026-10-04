use crate::config::{EngineConfig, ShadowConfig, SunDisk};
use crate::profile;

/// Angle horaire (radians) : 0 à midi, ±π à minuit. Calculé en f64 avant la
/// conversion, `time_of_day` étant stocké en f64.
pub fn hour_angle(time_of_day: f64) -> f32 {
    (std::f64::consts::TAU * (time_of_day - 0.5)) as f32
}

/// Direction de la lumière solaire (du soleil VERS la scène) à une heure donnée.
/// Repère : Z = haut, X = est, Y = nord. Trajectoire d'équinoxe (pas de saisons).
///
/// `time_of_day` ∈ [0, 1[ (0 = minuit, 0.5 = midi) ; `latitude` en radians :
/// à midi le soleil culmine à `90° - latitude`, plein sud (hémisphère nord).
pub fn sun_direction(time_of_day: f64, latitude: f32) -> glam::Vec3 {
    let (sin_h, cos_h) = hour_angle(time_of_day).sin_cos();
    let (sin_lat, cos_lat) = latitude.sin_cos();
    let to_sun = glam::Vec3::new(-sin_h, -sin_lat * cos_h, cos_lat * cos_h);
    -to_sun
}

pub struct DirectionalLight {
    pub direction: glam::Vec3,
    pub color: glam::Vec3,
    pub intensity: f32,
    pub disk: SunDisk,
    /// Boîte d'ombre. Portée par la lumière car son échelle dépend de la scène :
    /// `generate_terrain` remplace la boîte de petite scène par celle du terrain.
    pub shadow: ShadowConfig,
    /// Côté de la shadow map, fixé à sa création (cf. `ShadowMap::new`).
    pub(crate) shadow_map_resolution: u32,
}

impl DirectionalLight {
    /// `direction`, `color` et `intensity` sont des valeurs d'attente : recalculées
    /// d'après l'heure par `World::sync_sky` (dès `World::new`).
    pub(crate) fn new(config: &EngineConfig) -> DirectionalLight {
        let shadow = &config.render.shadow;
        DirectionalLight {
            direction: glam::Vec3::NEG_Z,
            color: glam::Vec3::ONE,
            intensity: 1.0,
            disk: config.environment.sun,
            shadow: shadow.static_scene.clone(),
            shadow_map_resolution: shadow.map_resolution,
        }
    }

    /// Côté d'un texel de la shadow map, en unités monde.
    pub fn shadow_texel_size(&self) -> f32 {
        self.shadow.texel_size(self.shadow_map_resolution)
    }

    /// Matrice view*proj de la lumière : on place une caméra orthographique le
    /// long de la direction du soleil, regardant le centre de la boîte d'ombre.
    /// C'est l'équivalent de `camera.view * camera.proj`, mais pour la lumière,
    /// et en projection orthographique (rayons parallèles = pas de perspective).
    ///
    /// Le centre dépend de [`ShadowConfig::follow_camera`] : soit l'origine
    /// (scène statique), soit un point devant la caméra (terrain) pour que la
    /// zone ombrée suive le joueur — `cam_pos`/`cam_front` servent à ce calcul.
    ///
    /// Note : volontairement PAS de flip de l'axe Y (contrairement à la caméra).
    /// La shadow map est rasterisée ET échantillonnée avec cette même matrice,
    /// donc le résultat reste cohérent ; inverser Y ici ne ferait que retourner
    /// la texture sans rien changer au calcul d'ombre.
    pub fn view_proj(&self, cam_pos: glam::Vec3, cam_front: glam::Vec3) -> glam::Mat4 {
        profile!();
        let dir = self.direction.normalize();
        let s = &self.shadow;

        let center = if s.follow_camera {
            cam_pos + cam_front * s.focus_distance
        } else {
            glam::Vec3::ZERO
        };
        let eye = center - dir * s.eye_distance;

        // « up » de la caméra-lumière : Z-up en général, sauf si la lumière est
        // quasi verticale (colinéaire à Z) — on bascule alors sur Y pour éviter
        // un look_at dégénéré.
        let up = if dir.cross(glam::Vec3::Z).length_squared() < 1e-4 {
            glam::Vec3::Y
        } else {
            glam::Vec3::Z
        };

        let view = glam::Mat4::look_at_rh(eye, center, up);
        // orthographic_rh (et non _gl) : profondeur clippée dans [0, 1], la
        // convention attendue par Vulkan.
        let proj = glam::Mat4::orthographic_rh(
            -s.half_size,
            s.half_size,
            -s.half_size,
            s.half_size,
            s.near,
            s.far,
        );
        proj * view
    }
}
