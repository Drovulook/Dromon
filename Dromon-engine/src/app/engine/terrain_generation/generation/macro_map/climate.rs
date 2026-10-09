//! **Climat** sur la grille climatique (plus grossière que la carte macro).
//!
//! La température est celle de l'altitude **moyenne** de chaque cellule. Le détail
//! devra la corriger localement : `T − lapse_rate · (h_détail − h_moyenne)`, sinon un
//! pic et la vallée voisine auraient la même température.

use noise::{NoiseFn, SuperSimplex};

use super::grid::{GridShape, MacroGrid};
use crate::config::ClimateParams;

// Décalages (espace bruit) : décorrélés des autres champs, qui partagent la graine.
const TEMP_OFF: f64 = 5233.7;
const WIND_ANGLE_OFF: f64 = 6091.3;
const WIND_SPEED_OFF: f64 = 6857.9;

/// Latitude normalisée : 0 au bord sud du monde, 1 au bord nord.
fn latitude(y: f64, radius: f64) -> f64 {
    ((y + radius) / (2.0 * radius)).clamp(0.0, 1.0)
}

/// Vecteur `(est, nord)` vers lequel souffle un vent venant du cap `from_deg`.
fn blowing_toward(from_deg: f64, speed: f64) -> (f64, f64) {
    // Souffle vers le cap opposé ; cap boussole → vecteur : 0° = nord (+Y), 90° = est (+X).
    let to = (from_deg + 180.0).to_radians();
    (to.sin() * speed, to.cos() * speed)
}

/// Vent dominant à la latitude `lat` : interpolation **des vecteurs** entre les
/// points de `wind_bands` (pas des angles : 60° → 240° passerait par 150°, un vent
/// qui n'existe pas ; les vecteurs opposés, eux, s'annulent en une bande calme).
fn band_wind(bands: &[(f64, f64, f64)], lat: f64) -> (f64, f64) {
    let (first, last) = (bands[0], bands[bands.len() - 1]);
    if lat <= first.0 || bands.len() == 1 {
        return blowing_toward(first.1, first.2);
    }
    if lat >= last.0 {
        return blowing_toward(last.1, last.2);
    }
    let k = bands.windows(2).position(|w| lat < w[1].0).unwrap_or(bands.len() - 2);
    let ((l0, d0, s0), (l1, d1, s1)) = (bands[k], bands[k + 1]);
    let t = (lat - l0) / (l1 - l0);
    let (a, b) = (blowing_toward(d0, s0), blowing_toward(d1, s1));
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

/// Vent `(est, nord)` sur `shape`, en multiples de la vitesse nominale : vent
/// dominant de sa latitude (`wind_bands`), tourné de ±`wind_deviation_deg` et
/// modulé en vitesse par deux bruits basse fréquence.
pub fn wind(
    seed: u32,
    params: &ClimateParams,
    radius: f64,
    shape: GridShape,
) -> (MacroGrid, MacroGrid) {
    let noise = SuperSimplex::new(seed);
    let f = params.wind_noise_frequency;
    let cells = shape.n * shape.n;
    let (mut east, mut north) = (Vec::with_capacity(cells), Vec::with_capacity(cells));
    for (i, j) in shape.cells() {
        let (x, y) = shape.cell_center(i, j);
        let (e, n) = band_wind(&params.wind_bands, latitude(y, radius));
        let turn = noise.get([x * f + WIND_ANGLE_OFF, y * f + WIND_ANGLE_OFF]);
        let gust = noise.get([x * f + WIND_SPEED_OFF, y * f + WIND_SPEED_OFF]);
        // Rotation du cap de δ (sens horaire, comme la boussole), puis vitesse.
        let (sin, cos) = (params.wind_deviation_deg * turn).to_radians().sin_cos();
        let speed = 1.0 + params.wind_speed_variation * gust;
        east.push(((e * cos + n * sin) * speed) as f32);
        north.push(((n * cos - e * sin) * speed) as f32);
    }
    (MacroGrid::from_vec(shape, east), MacroGrid::from_vec(shape, north))
}

/// Température (°C) imposée par la seule **latitude** (sud → nord). Équilibre de la
/// SST : sans le bruit de l'air, une anomalie de SST ne vient que d'un courant qui
/// amène de l'eau d'une autre latitude (sinon il décalerait juste le bruit).
pub fn latitude_temperature(params: &ClimateParams, radius: f64, shape: GridShape) -> MacroGrid {
    MacroGrid::from_fn(shape, |_, y| {
        let lat = latitude(y, radius);
        (params.south_temperature + (params.north_temperature - params.south_temperature) * lat)
            as f32
    })
}

/// Température (°C) de l'air **au niveau de la mer** : latitude + bruit basse
/// fréquence (variations locales). Base de [`temperature`].
pub fn sea_level_temperature(
    seed: u32,
    params: &ClimateParams,
    latitude_temp: &MacroGrid,
) -> MacroGrid {
    let noise = SuperSimplex::new(seed);
    let f = params.noise_frequency;
    let shape = latitude_temp.shape();
    MacroGrid::from_cells(shape, |i, j| {
        let (x, y) = shape.cell_center(i, j);
        let n = noise.get([x * f + TEMP_OFF, y * f + TEMP_OFF]);
        latitude_temp.get(i, j) + (params.noise_amplitude * n) as f32
    })
}

/// Température de l'air (°C) : celle du niveau de la mer, moins le refroidissement
/// avec l'altitude au-dessus de la mer.
pub fn temperature(
    params: &ClimateParams,
    sea_level_temp: &MacroGrid,
    sea_level: f64,
    relief: &MacroGrid,
) -> MacroGrid {
    sea_level_temp.zip_map(relief, |t, h| {
        t - (params.lapse_rate * (h as f64 - sea_level).max(0.0)) as f32
    })
}
