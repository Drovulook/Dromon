use glam::Vec3;

use crate::config::AtmosphereParams;

/// Atmosphère : réglages fixes (`environment.ron`) + couleurs du moment.
pub struct Atmosphere {
    pub params: AtmosphereParams,
    /// Couleur du ciel à l'horizon : aussi celle du brouillard sur le terrain
    /// (vu à l'horizontale ou en plongée) et la clear color.
    pub horizon_color: Vec3,
    /// Couleur du ciel au zénith.
    pub zenith_color: Vec3,
    /// Lumière indirecte du ciel (ambiant du terrain), multipliée par le matériau.
    pub ambient_color: Vec3,
}

impl Atmosphere {
    /// Les couleurs sont des valeurs d'attente : `day_night::apply` les recalcule
    /// d'après l'heure (dès `World::new`).
    pub fn new(params: AtmosphereParams) -> Atmosphere {
        Atmosphere {
            params,
            horizon_color: Vec3::ZERO,
            zenith_color: Vec3::ZERO,
            ambient_color: Vec3::ZERO,
        }
    }
}
