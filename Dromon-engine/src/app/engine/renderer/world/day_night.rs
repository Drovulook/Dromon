// Cycle jour/nuit : couleurs du ciel et de la lumière en fonction de l'élévation du
// soleil (pas de l'heure : reste juste si on change la latitude ou ajoute les saisons).

use glam::Vec3;

use crate::app::engine::renderer::world::atmosphere::Atmosphere;
use crate::app::engine::renderer::world::light::DirectionalLight;
use crate::app::engine::renderer::world::stars::Stars;

/// État du ciel pour une élévation solaire donnée. Couleurs linéaires.
struct SkyKeyframe {
    elevation_deg: f32,
    horizon: Vec3,
    zenith: Vec3,
    sun_color: Vec3,
    ambient: Vec3,
}

/// Triées par élévation croissante ; hors bornes, on garde la keyframe extrême.
const KEYFRAMES: [SkyKeyframe; 6] = [
    // nuit noire (fin du crépuscule astronomique)
    SkyKeyframe {
        elevation_deg: -18.0,
        horizon: Vec3::new(0.010, 0.012, 0.025),
        zenith: Vec3::new(0.003, 0.005, 0.015),
        sun_color: Vec3::new(1.0, 0.35, 0.10),
        ambient: Vec3::new(0.02, 0.025, 0.045),
    },
    // crépuscule civil : lueur violacée à l'horizon
    SkyKeyframe {
        elevation_deg: -6.0,
        horizon: Vec3::new(0.22, 0.13, 0.17),
        zenith: Vec3::new(0.03, 0.05, 0.14),
        sun_color: Vec3::new(1.0, 0.35, 0.10),
        ambient: Vec3::new(0.07, 0.07, 0.11),
    },
    // coucher / lever
    SkyKeyframe {
        elevation_deg: 0.0,
        horizon: Vec3::new(0.85, 0.45, 0.22),
        zenith: Vec3::new(0.12, 0.18, 0.42),
        sun_color: Vec3::new(1.0, 0.45, 0.15),
        ambient: Vec3::new(0.17, 0.16, 0.20),
    },
    // heure dorée
    SkyKeyframe {
        elevation_deg: 8.0,
        horizon: Vec3::new(0.72, 0.65, 0.62),
        zenith: Vec3::new(0.10, 0.23, 0.58),
        sun_color: Vec3::new(1.0, 0.72, 0.45),
        ambient: Vec3::new(0.24, 0.26, 0.33),
    },
    // après-midi
    SkyKeyframe {
        elevation_deg: 25.0,
        horizon: Vec3::new(0.55, 0.70, 0.88),
        zenith: Vec3::new(0.10, 0.25, 0.65),
        sun_color: Vec3::new(1.0, 0.85, 0.65),
        ambient: Vec3::new(0.28, 0.32, 0.40),
    },
    // soleil haut : lumière plus blanche
    SkyKeyframe {
        elevation_deg: 50.0,
        horizon: Vec3::new(0.55, 0.70, 0.90),
        zenith: Vec3::new(0.09, 0.24, 0.66),
        sun_color: Vec3::new(1.0, 0.93, 0.83),
        ambient: Vec3::new(0.29, 0.33, 0.41),
    },
];

/// Élévations (degrés) entre lesquelles l'éclairage direct s'allume. Sous -3°, le
/// disque (rayon ~1°) est entièrement caché par l'horizon : plus de lumière directe,
/// donc plus de halo ni d'éclairage par en dessous.
const SUN_FADE_START_DEG: f32 = -7.0;
const SUN_FADE_END_DEG: f32 = 1.0;

/// Élévations (degrés) du soleil entre lesquelles les étoiles s'effacent : visibles en
/// pleine nuit, disparues avant que le ciel ne s'éclaircisse franchement.
const STARS_FADE_START_DEG: f32 = -14.0;
const STARS_FADE_END_DEG: f32 = -5.0;

/// Met à jour lumière, atmosphère et étoiles d'après `light.direction` (déjà calculée
/// pour l'heure).
pub fn apply(light: &mut DirectionalLight, atmosphere: &mut Atmosphere, stars: &mut Stars) {
    let elevation_deg = (-light.direction.normalize().z).asin().to_degrees();

    let (a, b, t) = surrounding_keyframes(elevation_deg);
    atmosphere.horizon_color = a.horizon.lerp(b.horizon, t);
    atmosphere.zenith_color = a.zenith.lerp(b.zenith, t);
    atmosphere.ambient_color = a.ambient.lerp(b.ambient, t);
    light.color = a.sun_color.lerp(b.sun_color, t);
    light.intensity = smoothstep(SUN_FADE_START_DEG, SUN_FADE_END_DEG, elevation_deg);
    stars.visibility = 1.0 - smoothstep(STARS_FADE_START_DEG, STARS_FADE_END_DEG, elevation_deg);
}

/// Les deux keyframes qui encadrent `elevation_deg`, et la position `t` ∈ [0, 1] entre elles.
fn surrounding_keyframes(elevation_deg: f32) -> (&'static SkyKeyframe, &'static SkyKeyframe, f32) {
    let first = &KEYFRAMES[0];
    let last = &KEYFRAMES[KEYFRAMES.len() - 1];
    if elevation_deg <= first.elevation_deg {
        return (first, first, 0.0);
    }
    for pair in KEYFRAMES.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if elevation_deg <= b.elevation_deg {
            let t = (elevation_deg - a.elevation_deg) / (b.elevation_deg - a.elevation_deg);
            return (a, b, t);
        }
    }
    (last, last, 0.0)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
