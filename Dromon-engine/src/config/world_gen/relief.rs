//! Sections `coast` et `mountains` de `world_macro.ron` : le relief macro.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Altitude de base selon la distance à la côte `d` (voxels, > 0 terre).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoastProfile {
    /// Points `(d, altitude relative à sea_level)`, `d` strictement croissant, reliés
    /// par des segments ; constant au-delà des extrémités.
    pub points: Vec<(f64, f64)>,
}

impl Default for CoastProfile {
    fn default() -> Self {
        CoastProfile {
            points: vec![
                (-12000.0, -170.0),
                (-4000.0, -140.0),
                (-1500.0, -30.0),
                (0.0, 0.0),
                (3000.0, 30.0),
            ],
        }
    }
}

impl CoastProfile {
    pub(super) fn validate(&self) -> Result<()> {
        let p = &self.points;
        ensure!(!p.is_empty(), "coast.points ne doit pas être vide");
        ensure!(
            p.windows(2).all(|w| w[0].0 < w[1].0),
            "coast.points : les distances doivent être strictement croissantes"
        );
        Ok(())
    }
}

/// Chaînes de montagnes : où (axes), sur quelle largeur (piémont) et à quelle hauteur.
/// La **forme** du relief montagneux est côté détail.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MountainParams {
    /// Fréquence du bruit ridged dont les crêtes dessinent les axes des chaînes.
    pub frequency: f64,
    /// Axe de chaîne là où `1 − |bruit| > axis_threshold`, dans `[0, 1[`. Plus haut =
    /// chaînes plus fines et plus rares.
    pub axis_threshold: f64,
    /// Domain warping des axes : chaînes pliées plutôt que régulières.
    pub warp_frequency: f64,
    /// Déplacement maximal en voxels. `0.0` = désactivé.
    pub warp_amplitude: f64,
    /// Pas d'axe à moins de cette distance de la côte (voxels).
    pub min_coast_distance: f64,
    /// Largeur du piémont (voxels) : le masque passe de 1 sur l'axe à 0 à cette
    /// distance. Pente ajoutée ≈ 1,5 · amplitude / largeur.
    pub foothill_width: f64,
    /// Le masque s'annule à la côte et atteint son plein à cette distance (voxels) :
    /// les montagnes ne déplacent pas le trait de côte.
    pub coast_fade: f64,
    /// Hauteur des montagnes au-dessus du profil côtier (voxels).
    pub amplitude: f64,
}

impl Default for MountainParams {
    fn default() -> Self {
        MountainParams {
            frequency: 0.00003,
            axis_threshold: 0.9,
            warp_frequency: 0.00006,
            warp_amplitude: 3000.0,
            min_coast_distance: 1500.0,
            foothill_width: 4000.0,
            coast_fade: 1500.0,
            amplitude: 700.0,
        }
    }
}

impl MountainParams {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            (0.0..1.0).contains(&self.axis_threshold),
            "mountains.axis_threshold doit être dans [0, 1["
        );
        ensure!(
            self.foothill_width > 0.0 && self.coast_fade > 0.0,
            "mountains.foothill_width et coast_fade doivent être > 0"
        );
        Ok(())
    }
}
