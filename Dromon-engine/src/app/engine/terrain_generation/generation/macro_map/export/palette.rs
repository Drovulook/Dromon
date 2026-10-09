//! Couleurs et dégradés partagés par les exports.

use image::Rgb;

pub const SEA_SHALLOW: [f32; 3] = [120.0, 175.0, 225.0];
pub const SEA_DEEP: [f32; 3] = [10.0, 30.0, 90.0];
pub const LAND_LOW: [f32; 3] = [95.0, 165.0, 75.0];
pub const LAND_HIGH: [f32; 3] = [160.0, 125.0, 85.0];
pub const LAND_GREY: [f32; 3] = [120.0, 120.0, 120.0];

pub fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
}

pub fn to_rgb(c: [f32; 3], shade: f32) -> Rgb<u8> {
    Rgb(c.map(|v| (v * shade).clamp(0.0, 255.0) as u8))
}

/// Dégradé par paliers `(valeur, couleur)` triés, saturé aux extrémités.
pub fn ramp(stops: &[(f32, [f32; 3])], v: f32) -> [f32; 3] {
    let k = stops.windows(2).position(|w| v < w[1].0).unwrap_or(stops.len() - 2);
    let ((v0, c0), (v1, c1)) = (stops[k], stops[k + 1]);
    lerp(c0, c1, ((v - v0) / (v1 - v0)).clamp(0.0, 1.0))
}
