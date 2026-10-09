//! `world_macro.ron` : la struct racine et l'étendue du monde.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use super::{ClimateParams, CoastProfile, ContinentParams, MountainParams};
use crate::config::ConfigFile;

/// `world_macro.ron` : entrées de la carte macro.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldMacroConfig {
    pub seed: u32,
    pub world: WorldShape,
    /// Côté d'une cellule de la carte macro, en voxels.
    pub cell_size: u32,
    pub continents: ContinentParams,
    pub coast: CoastProfile,
    pub mountains: MountainParams,
    pub climate: ClimateParams,
}

impl Default for WorldMacroConfig {
    fn default() -> Self {
        WorldMacroConfig {
            seed: 0,
            world: WorldShape::default(),
            cell_size: 64,
            continents: ContinentParams::default(),
            coast: CoastProfile::default(),
            mountains: MountainParams::default(),
            climate: ClimateParams::default(),
        }
    }
}

impl ConfigFile for WorldMacroConfig {
    const FILE_NAME: &'static str = "world_macro.ron";

    fn validate(&self) -> Result<()> {
        ensure!(self.cell_size >= 1, "cell_size doit être ≥ 1");
        self.world.validate()?;
        self.continents.validate()?;
        self.coast.validate()?;
        self.mountains.validate()?;
        self.climate.validate(self.cell_size)
    }
}

/// Étendue du monde.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldShape {
    /// Rayon du monde, en chunks (cf. `WorldDisc`).
    pub radius_chunks: u32,
    /// Plafond du monde, en voxels : aucun relief n'est maillé au-dessus.
    pub max_height: u32,
    /// Niveau de la mer, en voxels.
    pub sea_level: f64,
}

impl Default for WorldShape {
    fn default() -> Self {
        WorldShape {
            radius_chunks: 4,
            max_height: 1024,
            sea_level: 200.0,
        }
    }
}

impl WorldShape {
    fn validate(&self) -> Result<()> {
        ensure!(self.radius_chunks > 0, "world.radius_chunks doit être > 0");
        ensure!(self.max_height >= 2, "world.max_height doit être ≥ 2");
        ensure!(
            self.sea_level >= 0.0 && self.sea_level < self.max_height as f64,
            "world.sea_level doit être dans [0, max_height["
        );
        Ok(())
    }
}
