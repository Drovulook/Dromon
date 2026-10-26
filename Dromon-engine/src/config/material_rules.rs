//! `material_rules.ron` : les **règles globales** des matériaux — placement
//! (`generation::material::rules`, CPU) et modulation des teintes (shader du terrain).
//! Ce qu'est chaque matériau (couleurs…) est dans `materials.ron`.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use super::ConfigFile;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MaterialRulesConfig {
    pub strata: StrataParams,
    pub snow_edge: ClimateEdge,
    pub snow_dryness: SnowDryness,
    pub desert_edge: ClimateEdge,
    pub sandstone_edge: ClimateEdge,
    pub outcrop: OutcropParams,
    pub shore: ShoreParams,
    pub dirt_patches: DirtPatchParams,
    pub slope: SlopeParams,
    pub noise: MaterialNoiseParams,
    pub coast_moisture: CoastMoisture,
}

/// Moins de neige là où il pleut peu : la limite des neiges est testée sur
/// `T + warming × sécheresse`, avec `sécheresse = 1 − smoothstep(dry, wet, pluie)`. La
/// frontière se **déplace** (plus haut en montagne sèche) au lieu de se diluer.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SnowDryness {
    /// Précipitations en dessous desquelles le décalage est plein.
    pub dry: f64,
    /// Précipitations au-dessus desquelles il est nul.
    pub wet: f64,
    /// Décalage (°C) en zone sèche : il y faut faire d'autant plus froid pour la neige.
    pub warming: f64,
}

impl Default for SnowDryness {
    fn default() -> Self {
        SnowDryness {
            dry: 0.05,
            wet: 0.30,
            warming: 4.0,
        }
    }
}

/// Humidité côtière ajoutée aux précipitations pour la **teinte** des matériaux
/// (`humidity_tint` : sable de plage beige, frange côtière plus verte). Ne change pas
/// leur placement. Envoyée au shader du terrain par l'UBO.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoastMoisture {
    /// Ajout aux précipitations (normalisées) sur la côte.
    pub amount: f32,
    /// Distance à la côte (voxels, vers les terres) où l'ajout s'annule.
    pub range: f32,
}

impl Default for CoastMoisture {
    fn default() -> Self {
        CoastMoisture {
            amount: 0.3,
            range: 200.0,
        }
    }
}

impl ConfigFile for MaterialRulesConfig {
    const FILE_NAME: &'static str = "material_rules.ron";

    fn validate(&self) -> Result<()> {
        let s = &self.strata;
        ensure!(
            0.0 < s.surface_depth && s.surface_depth < s.dirt_depth,
            "strata : il faut 0 < surface_depth < dirt_depth"
        );
        for (name, e) in [
            ("snow_edge", &self.snow_edge),
            ("desert_edge", &self.desert_edge),
            ("sandstone_edge", &self.sandstone_edge),
        ] {
            ensure!(
                e.blend >= 0.0 && e.blend_max_distance >= 0.0,
                "{name} : blend et blend_max_distance doivent être ≥ 0"
            );
            ensure!(
                e.jitter >= 0.0 && e.jitter_max_distance >= 0.0,
                "{name} : jitter et jitter_max_distance doivent être ≥ 0"
            );
        }
        let d = &self.snow_dryness;
        ensure!(d.dry < d.wet, "snow_dryness : il faut dry < wet");
        ensure!(d.warming >= 0.0, "snow_dryness.warming doit être ≥ 0");
        ensure!(self.outcrop.edge > 0.0, "outcrop.edge doit être > 0");
        let sh = &self.shore;
        ensure!(
            sh.beach_blend > 0.0 && sh.mud_blend > 0.0,
            "shore : beach_blend et mud_blend doivent être > 0"
        );
        let p = &self.dirt_patches;
        ensure!(
            p.start_above_snow > p.full_above_snow,
            "dirt_patches : il faut start_above_snow > full_above_snow"
        );
        ensure!(p.edge > 0.0, "dirt_patches.edge doit être > 0");
        let sl = &self.slope;
        ensure!(sl.macro_radius >= 1.0, "slope.macro_radius doit être ≥ 1");
        ensure!(
            sl.rock_none.degrees() < sl.rock_full.degrees()
                && sl.ledge_full.degrees() < sl.ledge_none.degrees()
                && sl.snow_rock_none.degrees() < sl.snow_rock_full.degrees(),
            "slope : il faut rock_none < rock_full, ledge_full < ledge_none, \
             snow_rock_none < snow_rock_full"
        );
        let n = &self.noise;
        for (name, layer) in [
            ("jitter", &n.jitter),
            ("dirt_patches", &n.dirt_patches),
            ("geology", &n.geology),
        ] {
            ensure!(layer.frequency > 0.0, "noise.{name}.frequency doit être > 0");
            // Ratio entier : les octaves s'alignent (motifs répétés).
            ensure!(
                layer.detail_ratio > 1.0 && layer.detail_ratio.fract() != 0.0,
                "noise.{name}.detail_ratio doit être > 1 et non entier"
            );
            ensure!(layer.detail_amp >= 0.0, "noise.{name}.detail_amp doit être ≥ 0");
        }
        ensure!(
            self.coast_moisture.amount >= 0.0 && self.coast_moisture.range > 0.0,
            "coast_moisture : il faut amount ≥ 0 et range > 0"
        );
        Ok(())
    }
}

/// Bruits des règles de matériaux (cf. `generation::material::noise`) : chacun = une
/// octave de forme + une octave de détail.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MaterialNoiseParams {
    /// Ondulation des frontières (neige, désert, plages…). Non normalisé : ~[−1,35 ; 1,35]
    /// avec `detail_amp` = 0,35, les amplitudes des frontières en tiennent compte.
    pub jitter: NoiseLayer,
    /// Plaques de terre, normalisé dans [−1, 1].
    pub dirt_patches: NoiseLayer,
    /// Géologie (massifs de roche normale en zone aride), normalisé dans [−1, 1].
    pub geology: NoiseLayer,
}

impl Default for MaterialNoiseParams {
    fn default() -> Self {
        MaterialNoiseParams {
            jitter: NoiseLayer {
                frequency: 0.01,
                detail_ratio: 4.1,
                detail_amp: 0.35,
            },
            dirt_patches: NoiseLayer {
                frequency: 0.001,
                detail_ratio: 2.7,
                detail_amp: 0.25,
            },
            geology: NoiseLayer {
                frequency: 0.0005,
                detail_ratio: 3.1,
                detail_amp: 0.2,
            },
        }
    }
}

/// Un bruit à deux octaves.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NoiseLayer {
    /// Fréquence de l'octave de forme (longueur d'onde ≈ 1 / fréquence voxels).
    pub frequency: f64,
    /// Fréquence de l'octave de détail / celle de forme. Non entier.
    pub detail_ratio: f64,
    /// Amplitude de l'octave de détail (forme = 1). Forte → îlots le long des bords.
    pub detail_amp: f64,
}

impl Default for NoiseLayer {
    fn default() -> Self {
        NoiseLayer {
            frequency: 0.001,
            detail_ratio: 2.7,
            detail_amp: 0.25,
        }
    }
}

/// Frontière douce sur une grandeur climatique (température, pluie). Jitter et
/// transition sont donnés dans l'unité de la grandeur **et** bornés en voxels au sol
/// (cf. `rules::ClimateEdge::above`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClimateEdge {
    /// Valeur de la frontière.
    pub threshold: f64,
    /// Largeur de la bande de transition, dans l'unité de la grandeur…
    pub blend: f64,
    /// … et au plus cette largeur en voxels au sol.
    pub blend_max_distance: f64,
    /// Amplitude du jitter de la frontière, dans l'unité de la grandeur…
    pub jitter: f64,
    /// … et au plus ce déplacement en voxels au sol.
    pub jitter_max_distance: f64,
}

impl Default for ClimateEdge {
    fn default() -> Self {
        ClimateEdge {
            threshold: 0.0,
            blend: 1.0,
            blend_max_distance: 40.0,
            jitter: 0.0,
            jitter_max_distance: 0.0,
        }
    }
}

/// Couches sous la surface (voxels de profondeur) : couverture, terre, puis roche.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StrataParams {
    pub surface_depth: f64,
    pub dirt_depth: f64,
}

impl Default for StrataParams {
    fn default() -> Self {
        StrataParams {
            surface_depth: 1.0,
            dirt_depth: 4.0,
        }
    }
}

/// Massifs de roche normale en zone aride, choisis par le bruit de géologie.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OutcropParams {
    /// Seuil du bruit (∈ [-1, 1]) au-dessus duquel la roche reste normale.
    pub threshold: f64,
    /// Demi-largeur de la transition, en unités de bruit.
    pub edge: f64,
}

impl Default for OutcropParams {
    fn default() -> Self {
        OutcropParams {
            threshold: 0.15,
            edge: 0.005,
        }
    }
}

/// Littoral et fonds marins (voxels).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShoreParams {
    /// Largeur des plages (distance à la côte).
    pub beach_width: f64,
    /// Largeur de la transition sable → couverture.
    pub beach_blend: f64,
    /// Amplitude du jitter de la largeur des plages.
    pub beach_jitter: f64,
    /// Profondeur sous la mer de la frontière sable → vase.
    pub mud_depth: f64,
    /// Largeur de la transition sable → vase.
    pub mud_blend: f64,
    /// Amplitude du jitter de cette frontière.
    pub seabed_jitter: f64,
}

impl Default for ShoreParams {
    fn default() -> Self {
        ShoreParams {
            beach_width: 80.0,
            beach_blend: 10.0,
            beach_jitter: 25.0,
            mud_depth: 120.0,
            mud_blend: 30.0,
            seabed_jitter: 15.0,
        }
    }
}

/// Plaques de terre dans l'herbe, en approchant de la limite des neiges.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirtPatchParams {
    /// Écart (°C) au-dessus de la limite des neiges où apparaissent les premières.
    pub start_above_snow: f64,
    /// Écart (°C) où la terre a remplacé toute l'herbe.
    pub full_above_snow: f64,
    /// Demi-largeur du bord adouci d'une plaque, en unités de bruit.
    pub edge: f64,
}

impl Default for DirtPatchParams {
    fn default() -> Self {
        DirtPatchParams {
            start_above_snow: 7.5,
            full_above_snow: 1.0,
            edge: 0.12,
        }
    }
}

/// Seuils de pente. Macro : moyenne sur `macro_radius`, donc plus douce que la pente
/// locale (normale), d'où des seuils plus bas.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SlopeParams {
    /// Rayon (voxels) de la mesure de pente macro : les reliefs plus étroits que
    /// `2 × rayon` n'y comptent pas.
    pub macro_radius: f64,
    /// Pente macro : roche pleine au-delà…
    pub rock_full: SlopeAngle,
    /// … aucune roche en deçà.
    pub rock_none: SlopeAngle,
    /// Pente locale : replat plein (la couverture reprend) en deçà…
    pub ledge_full: SlopeAngle,
    /// … plus de replat au-delà.
    pub ledge_none: SlopeAngle,
    /// En zone enneigée (la neige tient plus raide) : roche pleine au-delà…
    pub snow_rock_full: SlopeAngle,
    /// … aucune roche en deçà.
    pub snow_rock_none: SlopeAngle,
}

impl Default for SlopeParams {
    fn default() -> Self {
        SlopeParams {
            macro_radius: 12.0,
            rock_full: 47.0.into(),
            rock_none: 35.0.into(),
            ledge_full: 15.0.into(),
            ledge_none: 25.0.into(),
            snow_rock_full: 60.0.into(),
            snow_rock_none: 45.0.into(),
        }
    }
}

/// Angle de pente, écrit **en degrés** dans le fichier, lu en **cosinus** par les
/// règles (comparé à `normal.z`). Converti une fois au chargement : pas de `cos()`
/// par sommet.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "f64", into = "f64")]
pub struct SlopeAngle {
    degrees: f64,
    cos: f64,
}

impl SlopeAngle {
    pub fn degrees(self) -> f64 {
        self.degrees
    }

    pub fn cos(self) -> f64 {
        self.cos
    }
}

impl From<f64> for SlopeAngle {
    fn from(degrees: f64) -> Self {
        SlopeAngle {
            degrees,
            cos: degrees.to_radians().cos(),
        }
    }
}

impl From<SlopeAngle> for f64 {
    fn from(angle: SlopeAngle) -> f64 {
        angle.degrees
    }
}
