use noise::{NoiseFn, SuperSimplex};

use crate::app::engine::terrain_generation::utils::smoothstep;
use crate::config::ContinentParams;

// Décalages (espace bruit) : décorrélé du relief, qui partage la même graine.
const CONT_OFF: f64 = 1931.7;
const OCT_STEP: f64 = 97.31;
const BORDER_OFF: f64 = 2711.3;
/// Amplitude de la 2ᵉ octave de l'ondulation du bord (la 1ʳᵉ vaut 1).
const BORDER_OCT2_AMP: f64 = 0.25;

/// Continentalité `c(x, y)` : > 0 terre, < 0 mer. Seul le **signe** est fiable : la
/// valeur n'est pas une distance à la côte (cf. `distance.rs`).
pub struct ContinentField {
    noise: SuperSimplex,
    params: ContinentParams,
    /// Rayon du monde, en voxels.
    radius: f64,
}

impl ContinentField {
    pub fn new(seed: u32, params: ContinentParams, radius: f64) -> ContinentField {
        ContinentField {
            noise: SuperSimplex::new(seed),
            params,
            radius,
        }
    }

    pub fn value(&self, x: f64, y: f64) -> f64 {
        let p = &self.params;
        let (mut freq, mut amp) = (p.frequency, 1.0);
        let (mut sum, mut norm) = (0.0, 0.0);
        for o in 0..p.octaves {
            let off = CONT_OFF + o as f64 * OCT_STEP;
            sum += amp * self.noise.get([x * freq + off, y * freq + off]);
            norm += amp;
            freq *= p.lacunarity;
            amp *= p.gain;
        }
        sum / norm + p.land_bias - p.border_strength * self.border(x, y)
    }

    /// Océan de bord `∈ [0, 1]` : cache aussi la limite du monde. Le rayon est bruité
    /// vers l'extérieur seulement (`r' ≥ r`) : l'océan avance par endroits, mais reste
    /// garanti en `r = 1`.
    fn border(&self, x: f64, y: f64) -> f64 {
        let p = &self.params;
        let mut r = (x * x + y * y).sqrt() / self.radius;
        if p.border_noise > 0.0 {
            let f = p.border_noise_frequency;
            // Deux octaves (×1, ×4), divisées par la somme des amplitudes → ~[-1, 1] :
            // le `clamp` ne sature (bord plat) que sur des extrêmes rares.
            let n = (self.noise.get([x * f + BORDER_OFF, y * f + BORDER_OFF])
                + BORDER_OCT2_AMP
                    * self
                        .noise
                        .get([x * f * 4.0 + BORDER_OFF, y * f * 4.0 + BORDER_OFF]))
                / (1.0 + BORDER_OCT2_AMP);
            r += p.border_noise * ((n + 1.0) / 2.0).clamp(0.0, 1.0);
        }
        smoothstep(p.border_start, 1.0, r)
    }
}
