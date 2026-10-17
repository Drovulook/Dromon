//! Mélange de matériaux **de travail** : proportions en pleine précision, sans la
//! limite des 4 canaux du [`Voxel`], manipulées par les étapes du pipeline.

use super::super::super::chunk::{VOXEL_MATERIALS, Voxel};
use super::MATERIAL_AIR;

/// Poids en dessous duquel un matériau est ignoré à la quantification (résidus
/// flottants des `overlay`).
const MIN_WEIGHT: f64 = 1e-4;

/// Matériaux distincts suivis à la fois. Un point en mélange 4 ou 5 au plus : la
/// marge évite d'en perdre avant la quantification (qui n'en garde que
/// [`VOXEL_MATERIALS`]).
const CAPACITY: usize = 8;

/// Proportions des matériaux présents, en liste **creuse** `(id, poids)` : la taille
/// ne dépend pas du nombre de matériaux de `materials.ron`, et tout reste sur la pile
/// (aucune allocation sur le chemin chaud). Somme = 1.
#[derive(Clone, Copy, Debug)]
pub struct MaterialMix {
    ids: [u16; CAPACITY],
    weights: [f64; CAPACITY],
    len: usize,
}

impl MaterialMix {
    /// Proportion actuelle de `material` dans le mélange.
    pub fn weight(&self, material: u16) -> f64 {
        self.find(material).map_or(0.0, |i| self.weights[i])
    }

    /// Un seul matériau, à 100 %.
    pub fn solid(material: u16) -> Self {
        let mut mix = MaterialMix {
            ids: [MATERIAL_AIR; CAPACITY],
            weights: [0.0; CAPACITY],
            len: 0,
        };
        mix.add(material, 1.0);
        mix
    }

    /// Recouvre le mélange par `material` avec l'intensité `t ∈ [0, 1]` (calque
    /// d'opacité `t`). Préserve la somme : `(1 − t)·1 + t = 1`.
    pub fn overlay(&mut self, material: u16, t: f64) {
        let t = t.clamp(0.0, 1.0);
        for w in &mut self.weights[..self.len] {
            *w *= 1.0 - t;
        }
        self.add(material, t);
    }

    /// Convertit la fraction `t ∈ [0, 1]` de `from` en `to`, sans toucher aux autres
    /// matériaux (contrairement à [`MaterialMix::overlay`]). Préserve la somme.
    pub fn transfer(&mut self, from: u16, to: u16, t: f64) {
        let Some(i) = self.find(from) else {
            return;
        };
        let moved = self.weights[i] * t.clamp(0.0, 1.0);
        self.weights[i] -= moved;
        self.add(to, moved);
    }

    fn find(&self, material: u16) -> Option<usize> {
        self.ids[..self.len].iter().position(|&id| id == material)
    }

    /// Ajoute `w` au poids de `material`, en créant l'entrée au besoin. Liste pleine :
    /// l'entrée la plus faible est remplacée (son poids est perdu, `to_voxel`
    /// renormalise).
    fn add(&mut self, material: u16, w: f64) {
        if w <= 0.0 {
            return;
        }
        if let Some(i) = self.find(material) {
            self.weights[i] += w;
            return;
        }
        let slot = if self.len < CAPACITY {
            self.len += 1;
            self.len - 1
        } else {
            (0..CAPACITY)
                .min_by(|&a, &b| self.weights[a].total_cmp(&self.weights[b]))
                .unwrap()
        };
        self.ids[slot] = material;
        self.weights[slot] = w;
    }

    /// Quantifie vers le format stocké : les [`VOXEL_MATERIALS`] matériaux les plus
    /// présents, renormalisés, poids `u8` de somme exacte 255.
    pub fn to_voxel(&self) -> Voxel {
        // Entrées triées par poids décroissant ; on garde les premières non négligeables.
        let mut order: [usize; CAPACITY] = std::array::from_fn(|i| i);
        let order = &mut order[..self.len];
        order.sort_unstable_by(|&a, &b| self.weights[b].total_cmp(&self.weights[a]));
        let kept = order
            .iter()
            .take(VOXEL_MATERIALS)
            .take_while(|&&i| self.weights[i] > MIN_WEIGHT)
            .count();
        let order = &order[..kept];

        let mut voxel = Voxel::AIR;
        if kept == 0 {
            return voxel;
        }
        let total: f64 = order.iter().map(|&i| self.weights[i]).sum();

        // Le dernier canal reçoit le **reste** (255 − somme des précédents) pour un
        // total exact malgré les arrondis (sinon la couleur serait délavée).
        let mut assigned = 0u16;
        for (ch, &i) in order.iter().enumerate() {
            let q = if ch + 1 == kept {
                255u16.saturating_sub(assigned)
            } else {
                ((self.weights[i] / total) * 255.0).round() as u16
            };
            voxel.materials[ch] = self.ids[i];
            voxel.weights[ch] = q.min(255) as u8;
            assigned += q;
        }
        voxel
    }
}
