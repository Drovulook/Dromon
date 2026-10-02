use glam::Vec3;

pub struct Atmosphere {
    /// Couleur du ciel à l'horizon : aussi celle du brouillard sur le terrain
    /// (vu à l'horizontale ou en plongée) et la clear color.
    pub horizon_color: Vec3,
    /// Couleur du ciel au zénith.
    pub zenith_color: Vec3,
    /// Lumière indirecte du ciel (ambiant du terrain), multipliée par le matériau.
    pub ambient_color: Vec3,
    /// Part de l'ambiant renvoyée par le sol (∈ [0, 1]) : une face tournée vers le bas
    /// reçoit `ambient_color * ground_bounce`. Plus c'est bas, plus le relief ressort.
    pub ground_bounce: f32,
    /// Forme du dégradé horizon → zénith : `pow(élévation du regard, exposant)`.
    /// < 1 = transition resserrée près de l'horizon, comme un vrai ciel.
    pub sky_gradient_exponent: f32,
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

impl Default for Atmosphere {
    /// Les couleurs sont des valeurs d'attente : `day_night::apply` les recalcule
    /// d'après l'heure (dès `World::new`). Seuls les autres réglages sont fixes.
    fn default() -> Self {
        Atmosphere {
            horizon_color: Vec3::ZERO,
            zenith_color: Vec3::ZERO,
            ambient_color: Vec3::ZERO,
            ground_bounce: 0.25,
            sky_gradient_exponent: 0.75,
            fog_density: 0.0005,
            fog_scale_height: 1200.0,
            fog_anisotropy: 0.9,
            halo_strength: 0.003,
        }
    }
}
