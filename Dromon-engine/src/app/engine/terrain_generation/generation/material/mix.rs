//! Mélange de matériaux **de travail** : proportions en pleine précision, sans la
//! limite des 4 canaux du [`Voxel`], manipulées par les étapes du pipeline.

use super::super::super::chunk::Voxel;
use super::{MATERIAL_AIR, MATERIAL_COUNT};

/// Poids en dessous duquel un matériau est ignoré à la quantification (résidus
/// flottants des `overlay`).
const MIN_WEIGHT: f64 = 1e-4;

/// Proportion de chaque matériau, indexée par ID. Somme = 1.
#[derive(Clone, Copy, Debug)]
pub struct MaterialMix([f64; MATERIAL_COUNT]);

impl MaterialMix {
    /// Proportion actuelle de `material` dans le mélange.
    pub fn weight(&self, material: u16) -> f64 {
        self.0[material as usize]
    }

    /// Un seul matériau, à 100 %.
    pub fn solid(material: u16) -> Self {
        let mut w = [0.0; MATERIAL_COUNT];
        w[material as usize] = 1.0;
        MaterialMix(w)
    }

    /// Recouvre le mélange par `material` avec l'intensité `t ∈ [0, 1]` (calque
    /// d'opacité `t`). Préserve la somme : `(1 − t)·1 + t = 1`.
    pub fn overlay(&mut self, material: u16, t: f64) {
        let t = t.clamp(0.0, 1.0);
        for w in &mut self.0 {
            *w *= 1.0 - t;
        }
        self.0[material as usize] += t;
    }

    /// Quantifie vers le format stocké : les 4 matériaux les plus présents,
    /// renormalisés, poids `u8` de somme exacte 255.
    pub fn to_voxel(&self) -> Voxel {
        // IDs triés par poids décroissant ; on garde les 4 premiers non négligeables.
        let mut ids: [usize; MATERIAL_COUNT] = std::array::from_fn(|i| i);
        ids.sort_unstable_by(|&a, &b| self.0[b].total_cmp(&self.0[a]));
        let kept = ids
            .iter()
            .take(4)
            .take_while(|&&id| self.0[id] > MIN_WEIGHT)
            .count();
        let ids = &ids[..kept];

        let mut voxel = Voxel {
            materials: [MATERIAL_AIR; 4],
            weights: [0; 4],
        };
        let total: f64 = ids.iter().map(|&id| self.0[id]).sum();
        if kept == 0 {
            return voxel;
        }

        // Le dernier canal reçoit le **reste** (255 − somme des précédents) pour un
        // total exact malgré les arrondis (sinon la couleur serait délavée).
        let mut assigned = 0u16;
        for (ch, &id) in ids.iter().enumerate() {
            let q = if ch + 1 == kept {
                255u16.saturating_sub(assigned)
            } else {
                ((self.0[id] / total) * 255.0).round() as u16
            };
            voxel.materials[ch] = id as u16;
            voxel.weights[ch] = q.min(255) as u8;
            assigned += q;
        }
        voxel
    }
}
