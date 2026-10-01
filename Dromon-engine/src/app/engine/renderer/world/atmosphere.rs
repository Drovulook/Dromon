use glam::Vec3;

pub struct Atmosphere {
    // sert à la fois de clear color et de couleur du brouillard
    pub sky_color: Vec3,
    // σ de la loi de Beer-Lambert
    pub fog_density: f32,
    /// H — hauteur d'échelle : altitude à laquelle la densité est divisée par e.
    pub fog_scale_height: f32, // il faut H ≈ 3 × l'étendue verticale du terrain
    // brouillard teinté par le soleil
    /// g de Henyey-Greenstein ∈ [0,1[ : plus proche de 1 = halo plus concentré
    /// autour du soleil, mais pic beaucoup plus intense (compenser avec `halo_strength`).
    pub fog_anisotropy: f32,
    /// Intensité du halo solaire, dans le ciel ET dans le brouillard du terrain.
    pub halo_strength: f32,
}
