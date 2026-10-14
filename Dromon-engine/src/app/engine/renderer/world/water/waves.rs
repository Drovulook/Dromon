//! Houle de Gerstner : jeu de vagues fixe, tiré de `WaveParams` au démarrage et envoyé
//! dans l'UBO (évalué par `lib/water.slang`).

use glam::{Vec2, Vec4};
use std::f32::consts::TAU;

use crate::config::WaveParams;

/// ⚠ Synchronisé avec `WAVE_COUNT` (lib/bindings.slang).
pub(crate) const WAVE_COUNT: usize = 8;

/// Gravité (unités monde / s²), en supposant 1 unité = 1 m.
const GRAVITY: f32 = 9.81;

#[derive(Clone, Copy)]
struct Wave {
    direction: Vec2,
    /// Nombre d'onde `k = 2π / λ`.
    wavenumber: f32,
    amplitude: f32,
    /// Pulsation `ω = √(g·k)` (eau profonde) : les grandes vagues vont plus vite.
    angular_speed: f32,
    phase: f32,
}

pub(crate) struct Waves {
    waves: [Wave; WAVE_COUNT],
    coast_fade: f32,
}

impl Waves {
    pub fn new(params: &WaveParams) -> Waves {
        let base = params.direction_deg.to_radians();
        let spread = params.spread_deg.to_radians();
        let waves = std::array::from_fn(|i| {
            let wavelength = params.max_wavelength * params.wavelength_ratio.powi(i as i32);
            let k = TAU / wavelength;
            // Suites du nombre d'or : valeurs bien réparties dans [0, 1[, déterministes.
            // La première vague (la plus grande) suit exactement le vent (u = 0,5).
            let u = (0.5 + i as f32 * 0.618_034).fract();
            let phase = TAU * (i as f32 * 0.754_878).fract();
            Wave {
                direction: Vec2::from_angle(base + spread * (2.0 * u - 1.0)),
                wavenumber: k,
                // Même raideur k·A pour toutes : Σ k·A = steepness (pas de boucle si ≤ 1).
                amplitude: params.steepness / (WAVE_COUNT as f32 * k),
                angular_speed: params.speed * (GRAVITY * k).sqrt(),
                phase,
            }
        });
        Waves {
            waves,
            coast_fade: params.coast_fade,
        }
    }

    /// Écart maximal à la surface plane (toutes les crêtes alignées), vertical comme
    /// horizontal.
    pub fn max_amplitude(&self) -> f32 {
        self.waves.iter().map(|w| w.amplitude).sum()
    }

    /// Deux `vec4` par vague : (direction, k, A), (ω, phase, –, –). Puis les réglages
    /// communs : (coast_fade, –, –, –).
    pub fn gpu_data(&self) -> ([Vec4; 2 * WAVE_COUNT], Vec4) {
        let mut data = [Vec4::ZERO; 2 * WAVE_COUNT];
        for (i, w) in self.waves.iter().enumerate() {
            data[2 * i] = Vec4::new(w.direction.x, w.direction.y, w.wavenumber, w.amplitude);
            data[2 * i + 1] = Vec4::new(w.angular_speed, w.phase, 0.0, 0.0);
        }
        (data, Vec4::new(self.coast_fade, 0.0, 0.0, 0.0))
    }
}
