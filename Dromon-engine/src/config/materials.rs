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

/// IDs disponibles : `u16` (cf. `Voxel`), 0 réservé à l'air, le dernier au matériau
/// de debug.
const MAX_MATERIALS: usize = u16::MAX as usize - 1;

/// Textures du terrain : un sous-dossier par texture, aux cartes nommées
/// [`TEXTURE_COLOR`], [`TEXTURE_HEIGHT`]…
pub const TERRAIN_TEXTURE_DIR: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/res/textures/terrain_textures");
/// Albédo, sRGB.
pub const TEXTURE_COLOR: &str = "color.png";
/// Hauteur du relief (blanc = haut), pour le mélange par hauteur.
pub const TEXTURE_HEIGHT: &str = "height.png";

/// Chemin de la carte `file` de la texture `texture`.
pub fn terrain_texture_path(texture: &str, file: &str) -> PathBuf {
    Path::new(TERRAIN_TEXTURE_DIR).join(texture).join(file)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MaterialsConfig {
    pub materials: Vec<MaterialDef>,
}

/// Un matériau. Chaque propriété a une valeur par défaut : en ajouter une (rugosité,
/// texture…) ne casse pas les entrées existantes.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// Texture : sous-dossier de [`TERRAIN_TEXTURE_DIR`], partageable entre matériaux.
    /// `None` : couleur unie.
    pub texture: Option<String>,
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
            texture: None,
            texture_size: 10.0,
        }
    }
}

/// Fondu `dry_albedo` → `albedo` quand les précipitations (normalisées, 1 = côte
/// humide de référence) passent de `dry` à `wet`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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

    /// Textures distinctes, dans l'ordre de première apparition : la couche `i` du
    /// texture array est `textures()[i]`.
    pub fn textures(&self) -> Vec<&str> {
        let mut textures = Vec::new();
        for t in self.materials.iter().filter_map(|m| m.texture.as_deref()) {
            if !textures.contains(&t) {
                textures.push(t);
            }
        }
        textures
    }

    /// Couche du texture array de `def`, `None` s'il n'est pas texturé.
    pub fn texture_layer(&self, def: &MaterialDef) -> Option<u32> {
        let texture = def.texture.as_deref()?;
        self.textures()
            .iter()
            .position(|&t| t == texture)
            .map(|i| i as u32)
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
            if let Some(t) = &m.texture {
                for file in [TEXTURE_COLOR, TEXTURE_HEIGHT] {
                    let path = terrain_texture_path(t, file);
                    ensure!(
                        path.is_file(),
                        "{} : {} introuvable",
                        m.name,
                        path.display()
                    );
                }
            }
        }
        Ok(())
    }
}
