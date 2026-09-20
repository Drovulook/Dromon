use glam::Vec3;

pub struct Atmosphere {
    // sert à la fois de clear color et de couleur du brouillard
    pub sky_color: glam::Vec3,
    // σ de la loi de Beer-Lambert
    pub fog_density: f32,
}
