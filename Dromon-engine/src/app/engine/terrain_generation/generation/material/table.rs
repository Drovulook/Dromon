//! Table des matériaux **résolue** : `materials.ron` traduit une fois au démarrage en
//! IDs `u16`. Plus aucun nom sur le chemin chaud. Les propriétés de rendu (albédo…)
//! sont côté GPU (`MaterialBuffer`).

use anyhow::{Context, Result};

use crate::config::MaterialsConfig;

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
            debug: config.debug_id(),
        })
    }
}

/// Matériaux côté CPU, partagés avec les threads de maillage. Aujourd'hui les IDs
/// seuls ; accueillera les propriétés utiles à la génération ou au gameplay.
#[derive(Clone, Debug)]
pub struct MaterialTable {
    pub ids: MaterialIds,
}

impl MaterialTable {
    pub fn new(config: &MaterialsConfig) -> Result<MaterialTable> {
        Ok(MaterialTable {
            ids: MaterialIds::resolve(config)?,
        })
    }
}
