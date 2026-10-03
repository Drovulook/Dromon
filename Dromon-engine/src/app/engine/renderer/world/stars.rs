// Étoiles procédurales : dessinées par sky.slang, pilotées d'ici (visibilité, rotation
// du ciel, horloge de scintillation).

use glam::{Mat3, Vec3};

use crate::app::engine::renderer::world::light::hour_angle;

pub struct Stars {
    /// Luminosité (linéaire) des étoiles les plus brillantes.
    pub brightness: f32,
    /// Fraction des cases du ciel qui contiennent une étoile (grille : `STAR_CELLS`
    /// dans sky.slang).
    pub density: f32,
    /// Amplitude de la scintillation ∈ [0, 1] (doublée près de l'horizon).
    pub twinkle: f32,
    /// 0 le jour → 1 en pleine nuit. Recalculée par `day_night::apply`.
    pub(crate) visibility: f32,
    /// Rotation monde → ciel fixe : annule la rotation diurne pour que les étoiles
    /// restent attachées au ciel. Recalculée par `World::sync_sky`.
    pub(crate) world_to_sky: Mat3,
    /// Temps réel (s) : la scintillation ne suit pas l'accélération du temps de jeu.
    pub(crate) time_secs: f32,
}

impl Default for Stars {
    fn default() -> Self {
        Stars {
            brightness: 1.0,
            density: 0.03,
            twinkle: 0.25,
            visibility: 0.0,
            world_to_sky: Mat3::IDENTITY,
            time_secs: 0.0,
        }
    }
}

/// Rotation monde → ciel fixe à une heure donnée. Le ciel tourne autour du pôle
/// céleste `P` (au nord, élevé de `latitude`) d'un angle `-h` : `sun_direction` est
/// exactement cette rotation appliquée au soleil de midi. On l'annule avec `+h`.
pub fn world_to_sky(time_of_day: f64, latitude: f32) -> Mat3 {
    let pole = Vec3::new(0.0, latitude.cos(), latitude.sin());
    Mat3::from_axis_angle(pole, hour_angle(time_of_day))
}
