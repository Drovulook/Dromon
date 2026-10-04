// Cycle jour/nuit : couleurs du ciel et de la lumière en fonction de l'élévation du
// soleil (pas de l'heure : reste juste si on change la latitude ou ajoute les saisons).
// Keyframes et seuils : section `day_night` d'`environment.ron`.

use crate::app::engine::renderer::world::atmosphere::Atmosphere;
use crate::app::engine::renderer::world::light::DirectionalLight;
use crate::app::engine::renderer::world::stars::Stars;
use crate::config::{DayNightParams, SkyKeyframe};

/// Met à jour lumière, atmosphère et étoiles d'après `light.direction` (déjà calculée
/// pour l'heure).
pub fn apply(
    params: &DayNightParams,
    light: &mut DirectionalLight,
    atmosphere: &mut Atmosphere,
    stars: &mut Stars,
) {
    let elevation_deg = (-light.direction.normalize().z).asin().to_degrees();

    let (a, b, t) = surrounding_keyframes(&params.keyframes, elevation_deg);
    atmosphere.horizon_color = a.horizon.lerp(b.horizon, t);
    atmosphere.zenith_color = a.zenith.lerp(b.zenith, t);
    atmosphere.ambient_color = a.ambient.lerp(b.ambient, t);
    light.color = a.sun_color.lerp(b.sun_color, t);
    light.intensity = smoothstep(params.sun_fade_start_deg, params.sun_fade_end_deg, elevation_deg);
    stars.visibility =
        1.0 - smoothstep(params.stars_fade_start_deg, params.stars_fade_end_deg, elevation_deg);
}

/// Les deux keyframes qui encadrent `elevation_deg`, et la position `t` ∈ [0, 1] entre
/// elles. `keyframes` est non vide et trié (garanti par la validation de la config).
fn surrounding_keyframes(
    keyframes: &[SkyKeyframe],
    elevation_deg: f32,
) -> (&SkyKeyframe, &SkyKeyframe, f32) {
    let first = &keyframes[0];
    let last = &keyframes[keyframes.len() - 1];
    if elevation_deg <= first.elevation_deg {
        return (first, first, 0.0);
    }
    for pair in keyframes.windows(2) {
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
