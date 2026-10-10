//! Étapes du pipeline de matériaux, une fonction par règle.
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

use super::super::super::utils::smoothstep;
use super::MaterialQuery;
use super::mix::MaterialMix;
use super::{
    MATERIAL_DIRT, MATERIAL_GRASS, MATERIAL_MUD, MATERIAL_ROCK, MATERIAL_SAND, MATERIAL_SNOW,
};

// ─── Strates ─────────────────────────────────────────────────────────────────
/// Profondeur (en voxels sous la surface) de la couche de surface; au-dessous,
/// c'est de la terre puis de la roche.
const SURFACE_DEPTH: f64 = 1.0;
/// Profondeur jusqu'à laquelle on place de la terre ; au-delà, de la roche.
const DIRT_DEPTH: f64 = 4.0;

// ─── Frontières d'altitude de la couverture de surface ───────────────────────
/// Altitude de la frontière herbe → neige : au-dessus, neige.
const SNOW_BORDER: f64 = 740.0;
/// Largeur (en voxels d'altitude) de la bande de transition herbe → neige.
const BLEND_WIDTH: f64 = 40.0;

// ─── Littoral et fonds marins ────────────────────────────────────────────────
/// Largeur des plages (distance à la côte, voxels) : sable en deçà, herbe au-delà.
/// Le sable couvre aussi tout le fond marin (d < 0) jusqu'à la vase.
const BEACH_WIDTH: f64 = 40.0;
/// Largeur de la transition sable → herbe (voxels de distance à la côte).
const BEACH_BLEND: f64 = 20.0;
/// Profondeur sous la mer de la frontière sable → vase : le plateau continental
/// (≥ −30) reste sableux, le talus et les abysses sont vaseux.
const MUD_DEPTH: f64 = 40.0;
/// Largeur de la transition sable → vase (voxels de profondeur).
const MUD_BLEND: f64 = 30.0;

// ─── Plaques de terre ────────────────────────────────────────────────────────
/// Altitude des premières plaques de terre (rares, petites).
const PATCH_START: f64 = 350.0;
/// Altitude où la terre a remplacé toute l'herbe, juste sous la neige.
const PATCH_FULL: f64 = 600.0;
/// Demi-largeur (en unités de bruit) du bord adouci d'une plaque.
const PATCH_EDGE: f64 = 0.12;
/// Seuil du bruit à `PATCH_START` : au-dessus du bruit presque partout → aucune
/// plaque, puis les premiers pics émergent en montant.
const PATCH_THRESHOLD_START: f64 = 1.0 + PATCH_EDGE;
/// Seuil du bruit à `PATCH_FULL` : sous le bruit partout → terre pleine.
const PATCH_THRESHOLD_FULL: f64 = -1.0 - PATCH_EDGE;

// ─── Pentes ──────────────────────────────────────────────────────────────────
// Seuils en cos(pente) : `normal.z` et `macro_up` valent tous deux cos θ.
// La pente macro est une moyenne, donc plus douce que la pente locale : ses
// seuils sont plus bas.
/// Pente macro : roche pleine au-delà de ~47° (cos 47°).
const ROCK_SLOPE_FULL: f64 = 0.682;
/// Pente macro : aucune roche en dessous de ~35° (cos 35°).
const ROCK_SLOPE_NONE: f64 = 0.820;
/// Pente locale : replat plein (la couverture reprend) en dessous de ~15° (cos 15°).
const LEDGE_FULL: f64 = 0.966;
/// Pente locale : plus de replat au-delà de ~25° (cos 25°).
const LEDGE_NONE: f64 = 0.906;
/// Pente macro en zone enneigée : la neige tient plus raide, la roche
/// n'apparaît qu'au-delà de ~45° et devient pleine vers ~60°.
const SNOW_ROCK_SLOPE_FULL: f64 = 0.500; // cos 60°
const SNOW_ROCK_SLOPE_NONE: f64 = 0.707; // cos 45°

/// Couche de base : cascade par profondeur; couverture (altitude puis littoral) en
/// surface, puis terre, puis roche.
pub(super) fn base_layer(q: &MaterialQuery) -> MaterialMix {
    if q.depth < SURFACE_DEPTH {
        let mut mix = altitude_cover(q.mat_alt);
        shore(q, &mut mix);
        mix
    } else if q.depth < DIRT_DEPTH {
        MaterialMix::solid(MATERIAL_DIRT)
    } else {
        MaterialMix::solid(MATERIAL_ROCK)
    }
}

/// Couverture de surface selon l'altitude : herbe, recouverte de neige en haut à
/// travers la bande de transition.
fn altitude_cover(mat_alt: f64) -> MaterialMix {
    let half = BLEND_WIDTH * 0.5;
    let mut mix = MaterialMix::solid(MATERIAL_GRASS);
    mix.overlay(
        MATERIAL_SNOW,
        smoothstep(SNOW_BORDER - half, SNOW_BORDER + half, mat_alt),
    );
    mix
}

/// Littoral : sable sur les plages et le fond marin peu profond, vase au-delà du
/// plateau continental. Recouvre la couverture d'altitude (l'herbe ne pousse pas
/// sous l'eau).
fn shore(q: &MaterialQuery, mix: &mut MaterialMix) {
    let half = BEACH_BLEND * 0.5;
    mix.overlay(
        MATERIAL_SAND,
        1.0 - smoothstep(BEACH_WIDTH - half, BEACH_WIDTH + half, q.shore_d),
    );
    let half = MUD_BLEND * 0.5;
    mix.overlay(
        MATERIAL_MUD,
        smoothstep(MUD_DEPTH - half, MUD_DEPTH + half, q.sea_depth),
    );
}

/// Plaques de terre dans l'herbe, d'autant plus étendues que l'altitude est élevée.
///
/// Le bruit est un relief imaginaire noyé sous un niveau d'eau (le seuil) : les
/// plaques sont les îles qui émergent. En montant, le seuil baisse — les îles
/// grossissent, fusionnent, et à `PATCH_FULL` tout a émergé. Ne convertit que la
/// part d'**herbe** : sable et neige restent intacts.
pub(super) fn dirt_patches(q: &MaterialQuery, mix: &mut MaterialMix) {
    let Some(noise) = q.patch_noise else {
        return;
    };
    let a = ((q.mat_alt - PATCH_START) / (PATCH_FULL - PATCH_START)).clamp(0.0, 1.0);
    let threshold = PATCH_THRESHOLD_START + (PATCH_THRESHOLD_FULL - PATCH_THRESHOLD_START) * a;
    let patch = smoothstep(threshold - PATCH_EDGE, threshold + PATCH_EDGE, noise);
    mix.transfer(MATERIAL_GRASS, MATERIAL_DIRT, patch);
}

/// Grandes pentes → roche à nu, sauf sur les replats locaux (corniches) où la
/// couverture tient. Les petits ressauts raides sur terrain doux restent couverts.
/// En zone enneigée, les seuils glissent vers `SNOW_ROCK_SLOPE_*` (la neige tient
/// plus raide). Ne concerne que la surface exposée.
pub(super) fn slope_rock(q: &MaterialQuery, mix: &mut MaterialMix) {
    let (Some(n), Some(macro_up)) = (q.normal, q.macro_up) else {
        return;
    };

    // Part de neige posée par la couche de base : suit la limite de neige (jitter
    // et bande de transition compris), sans constante d'altitude en plus.
    let snow = mix.weight(MATERIAL_SNOW);
    let lerp = |a: f64, b: f64| a + (b - a) * snow;
    let full = lerp(ROCK_SLOPE_FULL, SNOW_ROCK_SLOPE_FULL);
    let none = lerp(ROCK_SLOPE_NONE, SNOW_ROCK_SLOPE_NONE);

    let steep = 1.0 - smoothstep(full, none, macro_up);
    let ledge = smoothstep(LEDGE_NONE, LEDGE_FULL, n.z as f64);
    mix.overlay(MATERIAL_ROCK, steep * (1.0 - ledge));
}
