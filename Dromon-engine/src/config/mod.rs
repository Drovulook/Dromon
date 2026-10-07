//! **Fichiers de configuration** du moteur (RON) : chargement, validation, écriture.
//!
//! Un fichier = une struct qui implémente [`ConfigFile`]. Ce module ne contient que des
//! données : les systèmes (terrain, rendu…) lisent ces structs, jamais l'inverse.
//!
//! Chaque struct porte `#[serde(default, deny_unknown_fields)]` :
//! - `default` : un champ absent prend sa valeur par défaut → les anciens fichiers se
//!   chargent encore après l'ajout d'un paramètre ;
//! - `deny_unknown_fields` : un champ inconnu est une **erreur** → une faute de frappe
//!   ne retombe pas silencieusement sur la valeur par défaut.

mod debug;
mod environment;
mod render;
mod world_gen;

pub use debug::{DebugConfig, MeshDebug};
pub use environment::{
    AtmosphereParams, ClockParams, DayNightParams, EnvironmentConfig, SkyKeyframe, StarsParams,
    SunDisk,
};
pub use render::{
    CameraParams, LodParams, PresentMode, RenderConfig, ShadowConfig, ShadowSettings,
    TerrainRenderParams,
};
pub use world_gen::{
    CoastProfile, ContinentParams, HeightParams, MountainParams, WorldDetailConfig, WorldGenConfig, WorldMacroConfig, WorldShape,
};

use anyhow::{Context, Result};
use serde::{Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};

/// Dossier `config/` à la racine du workspace. Résolu à la compilation : indépendant du
/// dossier d'où l'on lance le binaire.
pub const CONFIG_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../config");

/// Configs lues par le **moteur** au démarrage, avant la création du renderer
/// (`render.ron`, `environment.ron`, `debug.ron`). Les
/// configs propres à une scène (ex. [`WorldGenConfig`]) sont chargées par la scène.
#[derive(Clone, Debug)]
pub(crate) struct EngineConfig {
    pub render: RenderConfig,
    pub environment: EnvironmentConfig,
    pub debug: DebugConfig,
}

impl EngineConfig {
    pub fn load() -> Result<EngineConfig> {
        Ok(EngineConfig {
            render: RenderConfig::load()?,
            environment: EnvironmentConfig::load()?,
            debug: DebugConfig::load()?,
        })
    }
}

/// Une struct sérialisée dans son propre fichier RON de [`CONFIG_DIR`].
pub trait ConfigFile: Serialize + DeserializeOwned {
    /// Nom du fichier dans [`CONFIG_DIR`] (ex. `"render.ron"`).
    const FILE_NAME: &'static str;

    /// Rejette les valeurs qui casseraient le moteur (pas les réglages discutables).
    fn validate(&self) -> Result<()> {
        Ok(())
    }

    /// Chemin par défaut du fichier.
    fn default_path() -> PathBuf {
        Path::new(CONFIG_DIR).join(Self::FILE_NAME)
    }

    /// Charge et valide le fichier par défaut.
    fn load() -> Result<Self> {
        Self::load_from(Self::default_path())
    }

    /// Charge et valide un fichier quelconque.
    fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("lecture de {}", path.display()))?;
        let config: Self =
            ron::from_str(&text).with_context(|| format!("{} invalide", path.display()))?;
        config
            .validate()
            .with_context(|| format!("{} invalide", path.display()))?;
        Ok(config)
    }

    /// Écrit la config en RON indenté. ⚠ Les commentaires d'un fichier existant sont
    /// perdus : à réserver aux fichiers générés (valeurs par défaut, export).
    fn save_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .context("sérialisation RON")?;
        std::fs::write(path, text).with_context(|| format!("écriture de {}", path.display()))
    }
}
