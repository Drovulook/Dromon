//! `materials.ron` : la table des matériaux du terrain et leurs propriétés de rendu.
//!
//! L'ID d'un matériau est sa **position dans la liste + 1** (0 = air, réservé). Il n'est
//! jamais stocké sur disque : le code désigne les matériaux par leur `name`, résolu au
//! chargement. Réordonner la liste est donc sans risque.

use anyhow::{Result, ensure};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::ConfigFile;
use crate::AppDirs;

/// IDs disponibles : `u16` (cf. `Voxel`), 0 réservé à l'air, le dernier au matériau
/// de debug.
const MAX_MATERIALS: usize = u16::MAX as usize - 1;

/// Textures du terrain fournies (photos, exports) : un sous-dossier par texture, aux
/// cartes nommées [`TEXTURE_COLOR`], [`TEXTURE_HEIGHT`]…
pub fn terrain_texture_dir() -> PathBuf {
    AppDirs::get().res().join("textures/terrain_textures")
}

/// Textures du terrain écrites par la commande `texgen`, même disposition.
pub fn generated_texture_dir() -> PathBuf {
    AppDirs::get().work().join("texgen/terrain_textures")
}
/// Albédo, sRGB.
pub const TEXTURE_COLOR: &str = "color.png";
/// Hauteur du relief (blanc = haut), pour le mélange par hauteur.
pub const TEXTURE_HEIGHT: &str = "height.png";
/// Normal map en espace tangent, convention OpenGL (vert = +Y).
pub const TEXTURE_NORMAL: &str = "normal.png";
/// Occlusion ambiante (blanc = non occulté).
pub const TEXTURE_AO: &str = "ao.png";

/// Les 4 cartes lues par le moteur.
pub const TEXTURE_MAPS: [&str; 4] = [TEXTURE_COLOR, TEXTURE_HEIGHT, TEXTURE_NORMAL, TEXTURE_AO];

/// `dir` contient les 4 cartes.
pub fn has_all_maps(dir: &Path) -> bool {
    TEXTURE_MAPS.iter().all(|file| dir.join(file).is_file())
}

/// Origine de la texture d'un matériau : le nom désigne un sous-dossier de cartes. Un
/// dossier absent n'est pas une erreur de config : repli et avertissement au chargement
/// (cf. `TerrainTextures`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureSource {
    /// Couleur unie.
    #[default]
    None,
    /// Texture fournie : `res/textures/terrain_textures/<nom>/`.
    Files(String),
    /// Texture générée par la commande `texgen recipe=<nom>` :
    /// `work/texgen/terrain_textures/<nom>/`, à défaut le dossier de même nom de `res/`.
    Generated(String),
}

impl TextureSource {
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::Files(name) | Self::Generated(name) => Some(name),
        }
    }

    /// Dossiers où chercher les cartes, par ordre de préférence.
    pub fn candidate_dirs(&self) -> Vec<PathBuf> {
        match self {
            Self::None => Vec::new(),
            Self::Files(name) => vec![terrain_texture_dir().join(name)],
            Self::Generated(name) => vec![
                generated_texture_dir().join(name),
                terrain_texture_dir().join(name),
            ],
        }
    }

    /// Le dossier `name` de [`generated_texture_dir`] (`generated`) ou de
    /// [`terrain_texture_dir`] fait-il partie de [`Self::candidate_dirs`] ? Sert au hot
    /// reload : un dossier qu'aucune texture ne peut lire ne recharge rien.
    /// ⚠ Synchronisé avec `candidate_dirs`.
    pub fn may_read(&self, name: &str, generated: bool) -> bool {
        match self {
            Self::None => false,
            Self::Files(n) => !generated && n == name,
            Self::Generated(n) => n == name,
        }
    }
}

/// Nom de texture ou de recette, utilisé comme nom de dossier ou de fichier : rien qui
/// puisse en sortir (`/`, `..`).
pub fn is_valid_texture_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MaterialsConfig {
    pub materials: Vec<MaterialDef>,
}

/// Un matériau. Chaque propriété a une valeur par défaut : en ajouter une (rugosité,
/// texture…) ne casse pas les entrées existantes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MaterialDef {
    /// Identifiant utilisé par le code (règles de placement), unique.
    pub name: String,
    /// Couleur de base, **linéaire** (pas sRGB), composantes dans [0, 1]. Avec
    /// `humidity_tint`, c'est la couleur en zone humide.
    pub albedo: Vec3,
    /// Teinte selon les précipitations locales (herbe : verte → paille). `None` :
    /// couleur fixe.
    pub humidity_tint: Option<HumidityTint>,
    /// Texture, partageable entre matériaux. Défaut : couleur unie.
    pub texture: TextureSource,
    /// Côté d'une répétition de la texture, en unités monde (voxels).
    pub texture_size: f32,
}

impl Default for MaterialDef {
    fn default() -> Self {
        MaterialDef {
            name: String::new(),
            // Magenta : un albédo oublié saute aux yeux.
            albedo: Vec3::new(1.0, 0.0, 1.0),
            humidity_tint: None,
            texture: TextureSource::None,
            texture_size: 10.0,
        }
    }
}

/// Fondu `dry_albedo` → `albedo` quand les précipitations (normalisées, 1 = côte
/// humide de référence) passent de `dry` à `wet`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HumidityTint {
    /// Couleur en zone sèche, linéaire.
    pub dry_albedo: Vec3,
    /// Précipitations en dessous desquelles la couleur est `dry_albedo`.
    pub dry: f32,
    /// Précipitations au-dessus desquelles la couleur est `albedo`.
    pub wet: f32,
}

impl Default for HumidityTint {
    fn default() -> Self {
        HumidityTint {
            dry_albedo: Vec3::new(1.0, 0.0, 1.0),
            dry: 0.0,
            wet: 1.0,
        }
    }
}

impl MaterialsConfig {
    /// ID du matériau `name` (position + 1), `None` s'il n'existe pas.
    pub fn id_of(&self, name: &str) -> Option<u16> {
        self.materials
            .iter()
            .position(|m| m.name == name)
            .map(|i| (i + 1) as u16)
    }

    /// ID du matériau de **debug** (magenta, hors du fichier) : juste après le dernier.
    /// Sert aux teintes de `debug.ron` (cellules de transition).
    pub fn debug_id(&self) -> u16 {
        (self.materials.len() + 1) as u16
    }

    /// Textures distinctes (hors couleur unie), dans l'ordre de première apparition.
    /// Leurs couches dans le texture array : `TerrainTextures`, qui sait lesquelles
    /// ont pu être chargées.
    pub fn textures(&self) -> Vec<&TextureSource> {
        let mut textures = Vec::new();
        for m in &self.materials {
            if m.texture != TextureSource::None && !textures.contains(&&m.texture) {
                textures.push(&m.texture);
            }
        }
        textures
    }
}

impl ConfigFile for MaterialsConfig {
    const FILE_NAME: &'static str = "materials.ron";

    fn validate(&self) -> Result<()> {
        ensure!(!self.materials.is_empty(), "materials est vide");
        ensure!(
            self.materials.len() <= MAX_MATERIALS,
            "au plus {MAX_MATERIALS} matériaux (IDs u16, 0 = air, dernier = debug)"
        );
        let mut names = HashSet::new();
        for m in &self.materials {
            ensure!(!m.name.is_empty(), "un matériau n'a pas de name");
            ensure!(m.name != "air", "\"air\" est réservé (ID 0)");
            ensure!(names.insert(&m.name), "matériau \"{}\" en double", m.name);
            let in_unit = |c: Vec3| c.min_element() >= 0.0 && c.max_element() <= 1.0;
            ensure!(
                in_unit(m.albedo),
                "{} : albedo doit être dans [0, 1]",
                m.name
            );
            if let Some(t) = &m.humidity_tint {
                ensure!(
                    in_unit(t.dry_albedo),
                    "{} : humidity_tint.dry_albedo doit être dans [0, 1]",
                    m.name
                );
                ensure!(
                    t.dry < t.wet,
                    "{} : il faut humidity_tint.dry < humidity_tint.wet",
                    m.name
                );
            }
            ensure!(
                m.texture_size > 0.0,
                "{} : texture_size doit être > 0",
                m.name
            );
            // Fichiers absents : pas une erreur (repli au chargement), seul le nom compte.
            if let Some(name) = m.texture.name() {
                ensure!(
                    is_valid_texture_name(name),
                    "{} : nom de texture invalide \"{name}\" (attendu : [a-z0-9_-])",
                    m.name
                );
            }
        }
        Ok(())
    }
}
