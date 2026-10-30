//! **Dossiers de l'application** lancée : `<app>/config/` (RON, rechargés à chaud) et
//! `<app>/res/` (assets). Fixés une fois au démarrage, puis lus partout : c'est une
//! donnée du processus, comme le dossier courant.

use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static APP_DIRS: OnceLock<AppDirs> = OnceLock::new();

/// Dossiers d'une app, calculés une fois. Les ressources communes (shaders du renderer)
/// restent dans le moteur.
#[derive(Clone, Debug)]
pub struct AppDirs {
    config: PathBuf,
    res: PathBuf,
}

/// Dossiers du crate qui **invoque** la macro : `env!` est développé à la compilation de
/// l'app, pas du moteur.
#[macro_export]
macro_rules! app_dirs {
    () => {
        $crate::AppDirs::new(env!("CARGO_MANIFEST_DIR"))
    };
}

impl AppDirs {
    /// `root` : racine de l'app, qui contient `config/` et `res/`.
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            config: root.join("config"),
            res: root.join("res"),
        }
    }

    pub fn config(&self) -> &Path {
        &self.config
    }

    pub fn res(&self) -> &Path {
        &self.res
    }

    /// Fixe les dossiers du processus, une seule fois.
    pub fn install(self) -> Result<()> {
        ensure!(
            APP_DIRS.set(self).is_ok(),
            "dossiers de l'application déjà fixés"
        );
        Ok(())
    }

    /// Panique si [`AppDirs::install`] n'a pas été appelé : erreur de programmation.
    pub fn get() -> &'static AppDirs {
        APP_DIRS
            .get()
            .expect("AppDirs::install doit précéder tout accès aux fichiers de l'app")
    }
}
