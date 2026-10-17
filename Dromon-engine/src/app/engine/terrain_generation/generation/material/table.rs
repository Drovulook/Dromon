//! Table des matériaux **résolue** : `materials.ron` traduit une fois au démarrage en
//! IDs `u16`. Plus aucun nom sur le chemin chaud. Les propriétés de rendu (albédo…)
//! sont côté GPU (`MaterialBuffer`).

use anyhow::{Context, Result};

use super::noise::MaterialNoise;
use crate::config::{MaterialRulesConfig, MaterialsConfig};

/// IDs des matériaux que les règles de placement nomment. Un nom absent de
/// `materials.ron` est une erreur au démarrage.
#[derive(Clone, Copy, Debug)]
pub struct MaterialIds {
    pub rock: u16,
    pub dirt: u16,
    pub grass: u16,
    pub snow: u16,
    pub sand: u16,
    /// Vase des fonds marins, au-delà du plateau continental.
    pub mud: u16,
    /// Roche des zones arides (remplace `rock`).
    pub sandstone: u16,
    /// Magenta, hors du fichier (cf. [`MaterialsConfig::debug_id`]).
    pub debug: u16,
}

impl MaterialIds {
    pub fn resolve(config: &MaterialsConfig) -> Result<MaterialIds> {
        let id = |name: &str| {
            config
                .id_of(name)
                .with_context(|| format!("matériau \"{name}\" absent de materials.ron"))
        };
        Ok(MaterialIds {
            rock: id("rock")?,
            dirt: id("dirt")?,
            grass: id("grass")?,
            snow: id("snow")?,
            sand: id("sand")?,
            mud: id("mud")?,
            sandstone: id("sandstone")?,
            debug: config.debug_id(),
        })
    }
}

/// Matériaux côté CPU, partagés avec les threads de maillage : IDs, réglages et bruits
/// des règles de placement.
pub struct MaterialTable {
    pub ids: MaterialIds,
    pub rules: MaterialRulesConfig,
    pub noise: MaterialNoise,
}

impl MaterialTable {
    /// `seed` : graine du monde (bruits des règles).
    pub fn new(
        materials: &MaterialsConfig,
        rules: &MaterialRulesConfig,
        seed: u32,
    ) -> Result<MaterialTable> {
        Ok(MaterialTable {
            ids: MaterialIds::resolve(materials)?,
            rules: rules.clone(),
            noise: MaterialNoise::new(seed, rules.noise),
        })
    }
}
