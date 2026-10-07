//! Relief **macro** : `sea_level + profil(d) + w · amplitude`. Ce que le climat et les
//! lacs voient ; le détail y ajoutera la forme (mélange plaine / montagne par `w`).

use crate::config::{CoastProfile, MountainParams};

/// Altitude relative au niveau de la mer donnée par le profil côtier en `d`.
/// Interpolation linéaire : les cassures de pente aux points sont de quelques degrés
/// au plus, invisibles à l'éclairage.
pub fn coast_profile(profile: &CoastProfile, d: f64) -> f64 {
    let p = &profile.points;
    let (first, last) = (p[0], p[p.len() - 1]);
    if d <= first.0 {
        return first.1;
    }
    if d >= last.0 {
        return last.1;
    }
    // `p` est court (une poignée de points) : un parcours linéaire suffit.
    let k = p.windows(2).position(|w| d < w[1].0).unwrap_or(p.len() - 2);
    let ((d0, h0), (d1, h1)) = (p[k], p[k + 1]);
    h0 + (h1 - h0) * (d - d0) / (d1 - d0)
}

/// Altitude macro (voxels) pour une distance à la côte `d` et un masque de
/// montagne `w ∈ [0, 1]`.
pub fn macro_height(
    sea_level: f64,
    profile: &CoastProfile,
    mountains: &MountainParams,
    d: f64,
    w: f64,
) -> f64 {
    sea_level + coast_profile(profile, d) + w * mountains.amplitude
}
