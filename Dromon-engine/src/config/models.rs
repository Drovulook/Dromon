//! Source d'un modèle 3D, sur le modèle de [`TextureSource`](super::TextureSource).

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::materials::is_valid_texture_name;
use crate::AppDirs;

/// Fichier lu dans le dossier d'un modèle.
pub const MODEL_FILE: &str = "scene.gltf";

/// Modèles fournis : `res/models/<nom>/`.
pub fn model_dir() -> PathBuf {
    AppDirs::get().res().join("models")
}

/// Modèles écrits par la commande `objgen`, même disposition.
pub fn generated_model_dir() -> PathBuf {
    AppDirs::get().work().join("objgen/models")
}

/// Pas de `None` : un objet a toujours un modèle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelSource {
    /// Modèle fourni : `res/models/<nom>/`.
    Files(String),
    /// Modèle généré par `objgen` : `work/objgen/models/<nom>/`, à défaut le dossier
    /// de même nom de `res/`.
    Generated(String),
}

impl ModelSource {
    pub fn name(&self) -> &str {
        match self {
            Self::Files(name) | Self::Generated(name) => name,
        }
    }

    /// Fichiers où chercher le modèle, par ordre de préférence.
    pub fn candidate_paths(&self) -> Vec<PathBuf> {
        let file = |dir: PathBuf| dir.join(self.name()).join(MODEL_FILE);
        match self {
            Self::Files(_) => vec![file(model_dir())],
            Self::Generated(_) => vec![file(generated_model_dir()), file(model_dir())],
        }
    }

    /// Premier fichier existant. Nom hors `[a-z0-9_-]` refusé : il vient parfois d'une
    /// commande, et `../` sortirait des dossiers prévus.
    pub fn resolve(&self) -> Result<PathBuf> {
        if !is_valid_texture_name(self.name()) {
            bail!("nom de modèle invalide {:?} (attendu : [a-z0-9_-])", self.name());
        }
        let candidates = self.candidate_paths();
        if let Some(path) = candidates.iter().find(|path| path.is_file()) {
            return Ok(path.clone());
        }
        let tried: Vec<_> = candidates.iter().map(|p| p.display().to_string()).collect();
        bail!("modèle {self:?} introuvable (essayé : {})", tried.join(", "))
    }
}
