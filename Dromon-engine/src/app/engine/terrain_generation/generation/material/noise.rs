//! Bruits des règles de matériaux : ondulation des frontières, plaques de terre,
//! géologie. Réglages : `material_rules.ron` → `noise`.
//!
//! Fonctions des seules coordonnées **monde** (continues, sans couture entre chunks),
//! décorrélées entre elles et du relief par de grands décalages spatiaux. Chacun = une
//! octave de forme + une octave de détail qui dentelle les bords.

use noise::{NoiseFn, SuperSimplex};

use crate::config::{MaterialNoiseParams, NoiseLayer};

// Décalages monde qui décorrèlent les bruits entre eux (même source).
const JITTER_OFFSET: f64 = 4096.0;
const DIRT_PATCH_OFFSET: f64 = 8192.0;
const GEOLOGY_OFFSET: f64 = 12288.0;

pub struct MaterialNoise {
    noise: SuperSimplex,
    params: MaterialNoiseParams,
}

impl MaterialNoise {
    pub fn new(seed: u32, params: MaterialNoiseParams) -> MaterialNoise {
        MaterialNoise {
            noise: SuperSimplex::new(seed),
            params,
        }
    }

    /// Les deux octaves de `layer` en `(wx, wy)`, décalées de `offset` : `(forme,
    /// détail)`.
    fn octaves(&self, layer: &NoiseLayer, offset: f64, wx: f64, wy: f64) -> (f64, f64) {
        let (x, y) = (wx + offset, wy + offset);
        let f = layer.frequency;
        let d = f * layer.detail_ratio;
        (self.noise.get([x * f, y * f]), self.noise.get([x * d, y * d]))
    }

    /// Bruit unitaire qui fait onduler les frontières : chaque règle le met à son
    /// échelle. On perturbe la grandeur *testée*, pas le seuil : la frontière devient
    /// l'iso-courbe de `valeur + jitter`. Non normalisé (~±(1 + detail_amp)).
    pub fn jitter(&self, wx: f64, wy: f64) -> f64 {
        let layer = &self.params.jitter;
        let (shape, detail) = self.octaves(layer, JITTER_OFFSET, wx, wy);
        shape + layer.detail_amp * detail
    }

    /// Bruit des **plaques de terre**, normalisé dans `[-1, 1]` (cf.
    /// `rules::dirt_patches`, qui le seuille).
    ///
    /// Divisé par la somme des amplitudes : borne garantie, pas seulement typique — un
    /// seuil ≥ 1 + bord adouci n'émet donc **aucune** plaque. Revers : les extrêmes
    /// (les deux octaves culminant ensemble) sont rares, l'essentiel tient dans ~±0,6.
    pub fn dirt_patch(&self, wx: f64, wy: f64) -> f64 {
        normalized(&self.params.dirt_patches, self.octaves(&self.params.dirt_patches, DIRT_PATCH_OFFSET, wx, wy))
    }

    /// Bruit de **géologie**, normalisé dans `[-1, 1]` : choisit la roche locale (cf.
    /// `rules::slope_rock` : roche normale au lieu du grès en zone aride). Des massifs
    /// entiers : grande longueur d'onde, détail faible (sinon îlots le long des bords).
    pub fn geology(&self, wx: f64, wy: f64) -> f64 {
        normalized(&self.params.geology, self.octaves(&self.params.geology, GEOLOGY_OFFSET, wx, wy))
    }
}

/// `(forme + a · détail) / (1 + a)` : dans `[-1, 1]` quelles que soient les octaves.
fn normalized(layer: &NoiseLayer, (shape, detail): (f64, f64)) -> f64 {
    (shape + layer.detail_amp * detail) / (1.0 + layer.detail_amp)
}
