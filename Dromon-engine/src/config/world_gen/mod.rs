//! Génération du monde, en deux fichiers :
//! - `world_macro.ron` : la **carte macro** précalculée (graine, étendue, continents,
//!   relief macro, climat) ;
//! - `world_detail.ron` : le **détail fin** calculé à la demande (relief fBm…).
//!
//! Le détail lit la macro, jamais l'inverse : les champs communs (graine, étendue)
//! vivent donc côté macro. Modifier `world_detail.ron` n'invalide pas la carte macro.
//!
//! Un fichier par section de `world_macro.ron`, chacune avec sa propre `validate`.

mod climate;
mod continents;
mod detail;
mod macro_config;
mod relief;

pub use climate::ClimateParams;
pub use continents::ContinentParams;
pub use detail::{HeightParams, WorldDetailConfig};
pub use macro_config::{WorldMacroConfig, WorldShape};
pub use relief::{CoastProfile, MountainParams};

use anyhow::Result;

use super::ConfigFile;

/// Les deux fichiers réunis : ce que la scène passe au terrain.
#[derive(Clone, Debug, Default)]
pub struct WorldGenConfig {
    // `macro` est un mot-clé Rust, d'où le `_` final.
    pub macro_: WorldMacroConfig,
    pub detail: WorldDetailConfig,
}

impl WorldGenConfig {
    /// Charge et valide les deux fichiers par défaut.
    pub fn load() -> Result<WorldGenConfig> {
        Ok(WorldGenConfig {
            macro_: WorldMacroConfig::load()?,
            detail: WorldDetailConfig::load()?,
        })
    }
}
