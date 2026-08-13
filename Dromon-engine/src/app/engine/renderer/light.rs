use crate::profile;

// Paramètres du « frustum » orthographique de la lumière (la boîte qui doit
// englober toute la scène projetant des ombres). La boîte SUIT la caméra
// ; ces valeurs fixent sa taille, pas sa position.
//
// Compromis résolution : la shadow map (2048²) est étalée sur `2*HALF_SIZE`
// unités → ~`2*HALF_SIZE / 2048` u/texel. Plus la boîte est grande, plus on
// couvre de terrain mais plus les ombres deviennent grossières. ~150 ⇒ ~0.15
// u/texel, correct à l'échelle voxel.
/// Réglages du « frustum » orthographique de la shadow map. Portés par la scène
/// (via [`DirectionalLight`]) car l'échelle dépend du contenu : une petite scène
/// d'objets fixes et un terrain géant qu'on survole ne veulent ni la même taille
/// de boîte, ni la même stratégie de centrage.
///
/// La shadow map (2048²) est étalée sur `2 * half_size` unités → résolution
/// `2 * half_size / 2048` u/texel : plus la boîte est grande, plus on couvre de
/// monde mais plus les ombres deviennent grossières.
pub struct ShadowConfig {
    /// Demi-largeur/hauteur de la boîte orthographique, en unités monde.
    pub half_size: f32,
    /// Plans near/far le long de l'axe lumière, depuis l'« œil » virtuel.
    /// ⚠ `far` élargit aussi le biais anti-acné (exprimé en profondeur
    /// normalisée) : un `far` énorme sur de petits objets décolle leur ombre.
    pub near: f32,
    pub far: f32,
    /// Recul de l'« œil » virtuel le long de `-direction`.
    pub eye_distance: f32,
    /// Si `true`, la boîte suit la caméra (indispensable pour un grand terrain) ;
    /// sinon elle reste centrée sur l'origine (idéal pour une scène d'objets
    /// fixes autour de l'origine).
    pub follow_camera: bool,
    /// Quand `follow_camera`, distance devant la caméra (le long du regard) du
    /// point de centrage : on dépense le budget d'ombre là où le joueur regarde.
    pub focus_distance: f32,
}

impl Default for ShadowConfig {
    /// Défauts pour une **petite scène statique** centrée sur l'origine.
    /// `generate_terrain` les remplace par des valeurs
    /// adaptées au terrain.
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

pub struct DirectionalLight {
    pub direction: glam::Vec3,
    pub color: glam::Vec3,
    pub intensity: f32,
    pub shadow: ShadowConfig,
}

impl DirectionalLight {
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
