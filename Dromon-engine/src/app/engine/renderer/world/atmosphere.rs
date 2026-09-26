use glam::Vec3;

pub struct Atmosphere {
    // sert à la fois de clear color et de couleur du brouillard
    pub sky_color: Vec3,
    // σ de la loi de Beer-Lambert
    pub fog_density: f32,
    /// H — hauteur d'échelle : altitude à laquelle la densité est divisée par e.
    pub fog_scale_height: f32, // il faut H ≈ 3 × l'étendue verticale du terrain
    // brouillard teinté par le soleil
    pub fog_anisotropy: f32,
}
