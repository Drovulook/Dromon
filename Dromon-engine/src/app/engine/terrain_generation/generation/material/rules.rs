//! Étapes du pipeline de matériaux, une fonction par règle. Réglages :
//! `config/material_rules.ron` (cf. [`MaterialRulesConfig`]), lus via `q.rules`.
//!
//! - [`base_layer`] crée le mélange initial ;
//! - les modificateurs ont la forme `fn(&MaterialQuery, &mut MaterialMix)` et
//!   recouvrent une part du mélange via [`MaterialMix::overlay`].
//!
//! ## Transitions douces
//! Un seuil franc (`alt > BORDER`) crée une frontière nette. Chaque règle ouvre
//! plutôt une bande de transition où l'intensité du recouvrement monte en
//! `smoothstep` ; avec l'interpolation du rasterizer entre sommets, on obtient un
//! dégradé au lieu d'un trait.
//!
//! [`MaterialRulesConfig`]: crate::config::MaterialRulesConfig

use super::super::super::utils::smoothstep;
use super::MaterialQuery;
use super::mix::MaterialMix;
use crate::config::ClimateEdge;

/// Part ∈ [0, 1] du côté **haut** de la frontière climatique `edge`, pour `value` de
/// gradient `gradient` et le bruit unitaire `jitter`.
///
/// Jitter et transition sont bornés **en distance au sol** : un écart Δ de la grandeur
/// déplace la frontière de `Δ / |∇|`. En plaine, où le climat varie à peine (T :
/// ~3·10⁻⁴ °C/voxel), un jitter en valeur la déplacerait de milliers de voxels →
/// mosaïque de taches. On garde donc `min(amplitude, distance max × |∇|)` : là où le
/// climat varie vite (montagne), l'amplitude l'emporte.
fn above(edge: &ClimateEdge, value: f64, gradient: f64, jitter: f64) -> f64 {
    let v = value + jitter * edge.jitter.min(edge.jitter_max_distance * gradient);
    // Plancher : un smoothstep de largeur nulle diviserait 0 par 0.
    let half = (edge.blend.min(edge.blend_max_distance * gradient) * 0.5).max(1e-9);
    smoothstep(edge.threshold - half, edge.threshold + half, v)
}

/// Couche de base : cascade par profondeur; couverture (climat puis littoral) en
/// surface, puis terre, puis roche.
pub(super) fn base_layer(q: &MaterialQuery) -> MaterialMix {
    let strata = &q.rules.strata;
    if q.depth < strata.surface_depth {
        let mut mix = climate_cover(q);
        shore(q, &mut mix);
        mix
    } else if q.depth < strata.dirt_depth {
        MaterialMix::solid(q.ids.dirt)
    } else {
        MaterialMix::solid(q.ids.rock)
    }
}

/// Couverture de surface selon le climat : herbe, remplacée par le sable dans les
/// déserts, puis recouverte de neige quand il fait froid. En zone sèche, la limite des
/// neiges est testée sur une température relevée (`snow_dryness`) : il y faut faire
/// plus froid, la limite monte en montagne et quitte les plaines sèches.
fn climate_cover(q: &MaterialQuery) -> MaterialMix {
    let c = &q.climate;
    let mut mix = MaterialMix::solid(q.ids.grass);
    let r = q.rules;
    let desert = 1.0 - above(&r.desert_edge, c.precipitation, c.precipitation_gradient, q.jitter);
    mix.transfer(q.ids.grass, q.ids.sand, desert);
    let d = &r.snow_dryness;
    let dryness = 1.0 - smoothstep(d.dry, d.wet, c.precipitation);
    let snow_t = c.temperature + d.warming * dryness;
    let snow = 1.0 - above(&r.snow_edge, snow_t, c.temperature_gradient, q.jitter);
    mix.overlay(q.ids.snow, snow);
    mix
}

/// Littoral : sable sur les plages et le fond marin peu profond, vase au-delà du
/// plateau continental. Recouvre la couverture d'altitude (l'herbe ne pousse pas
/// sous l'eau).
fn shore(q: &MaterialQuery, mix: &mut MaterialMix) {
    let s = &q.rules.shore;
    let half = s.beach_blend * 0.5;
    mix.overlay(
        q.ids.sand,
        1.0 - smoothstep(s.beach_width - half, s.beach_width + half, q.shore_d),
    );
    let half = s.mud_blend * 0.5;
    mix.overlay(
        q.ids.mud,
        smoothstep(s.mud_depth - half, s.mud_depth + half, q.sea_depth),
    );
}

/// Plaques de terre dans l'herbe, d'autant plus étendues qu'il fait froid.
///
/// Le bruit est un relief imaginaire noyé sous un niveau d'eau (le seuil) : les
/// plaques sont les îles qui émergent. En refroidissant, le seuil baisse — les îles
/// grossissent, fusionnent, et à `full_above_snow` tout a émergé. Ne convertit que la
/// part d'**herbe** : sable et neige restent intacts.
pub(super) fn dirt_patches(q: &MaterialQuery, mix: &mut MaterialMix) {
    let Some(noise) = q.patch_noise else {
        return;
    };
    let p = &q.rules.dirt_patches;
    let start = q.rules.snow_edge.threshold + p.start_above_snow;
    let full = q.rules.snow_edge.threshold + p.full_above_snow;
    // Seuil du bruit : au-dessus du bruit partout au début (aucune plaque), sous le
    // bruit partout à la fin (terre pleine). Bruit ∈ [-1, 1], bord adouci compris.
    let (threshold_start, threshold_full) = (1.0 + p.edge, -1.0 - p.edge);

    let a = ((q.climate.temperature - start) / (full - start)).clamp(0.0, 1.0);
    let threshold = threshold_start + (threshold_full - threshold_start) * a;
    let patch = smoothstep(threshold - p.edge, threshold + p.edge, noise);
    mix.transfer(q.ids.grass, q.ids.dirt, patch);
}

/// Grandes pentes → roche à nu, sauf sur les replats locaux (corniches) où la
/// couverture tient. Les petits ressauts raides sur terrain doux restent couverts.
/// En zone enneigée, les seuils glissent vers `snow_rock_*` (la neige tient plus
/// raide). En zone aride, la roche est du grès, sauf sur les massifs que le bruit de
/// géologie garde en roche normale. Ne concerne que la surface exposée.
///
/// Seuils comparés en cos(pente) : `normal.z` et `macro_up` valent tous deux cos θ.
pub(super) fn slope_rock(q: &MaterialQuery, mix: &mut MaterialMix) {
    let (Some(n), Some(macro_up)) = (q.normal, q.macro_up) else {
        return;
    };
    let s = &q.rules.slope;

    // Part de neige posée par la couche de base : suit la limite de neige (jitter
    // et bande de transition compris), sans constante d'altitude en plus.
    let snow = mix.weight(q.ids.snow);
    let lerp = |a: f64, b: f64| a + (b - a) * snow;
    let full = lerp(s.rock_full.cos(), s.snow_rock_full.cos());
    let none = lerp(s.rock_none.cos(), s.snow_rock_none.cos());

    let steep = 1.0 - smoothstep(full, none, macro_up);
    let ledge = smoothstep(s.ledge_none.cos(), s.ledge_full.cos(), n.z as f64);
    mix.overlay(q.ids.rock, steep * (1.0 - ledge));

    let c = &q.climate;
    let arid =
        1.0 - above(&q.rules.sandstone_edge, c.precipitation, c.precipitation_gradient, q.jitter);
    let o = &q.rules.outcrop;
    let outcrop = q
        .geology_noise
        .map_or(0.0, |n| smoothstep(o.threshold - o.edge, o.threshold + o.edge, n));
    mix.transfer(q.ids.rock, q.ids.sandstone, arid * (1.0 - outcrop));
}
