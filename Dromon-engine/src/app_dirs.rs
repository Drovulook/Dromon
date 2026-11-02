//! **Dossiers de l'application** lancée : `<app>/config/` (RON, rechargés à chaud),
//! `<app>/dev_res/` (sources des assets), `<app>/res/` (assets) et `<app>/work/`
//! (intermédiaires). Fixés une fois au démarrage, puis lus partout : c'est une donnée du
//! processus, comme le dossier courant.

use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static APP_DIRS: OnceLock<AppDirs> = OnceLock::new();

/// Dossiers d'une app, calculés une fois. Les ressources communes (shaders du renderer)
/// restent dans le moteur.
#[derive(Clone, Debug)]
pub struct AppDirs {
    config: PathBuf,
    dev_res: PathBuf,
    res: PathBuf,
    work: PathBuf,
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
    /// `root` : racine de l'app, qui contient `config/`, `dev_res/`, `res/` et `work/`.
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            config: root.join("config"),
            dev_res: root.join("dev_res"),
            res: root.join("res"),
            work: root.join("work"),
        }
    }

    pub fn config(&self) -> &Path {
        &self.config
    }

    /// Sources des assets (recettes texgen…), versionnées : le moteur les compile ou les
    /// exécute pour produire `res/` ou `work/`.
    pub fn dev_res(&self) -> &Path {
        &self.dev_res
    }

    /// Ce que le moteur lit en jeu (textures, modèles, caches).
    pub fn res(&self) -> &Path {
        &self.res
    }

    /// Sorties intermédiaires pour l'humain ou l'agent (exports PNG…) : jetables, jamais
    /// lues par le moteur.
    pub fn work(&self) -> &Path {
        &self.work
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
