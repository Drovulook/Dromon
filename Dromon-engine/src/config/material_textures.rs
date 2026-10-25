//! `material_textures.ron` : rendu des textures du terrain (mélanges, normal maps, AO).
//! Lu par le shader via l'UBO (`texture_blend`, `texture_detail`).

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use super::ConfigFile;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MaterialTexturesConfig {
    /// Exposant des poids triplanaires `|N|^k` : grand = fondu étroit entre projections.
    pub triplanar_sharpness: f32,
    /// Profondeur du mélange par hauteur entre projections : → 0 frontière découpée,
    /// grand = moyenne pondérée (surimpression).
    pub projection_blend_depth: f32,
    /// Idem entre matériaux.
    pub material_blend_depth: f32,
    /// Multiplie les pentes des normal maps (0 = sans relief).
    pub normal_strength: f32,
    /// Force de l'AO des textures sur l'ambiant (0 = ignorée, 1 = telle que fournie).
    pub ao_strength: f32,
    /// Hauteur de relief prêtée aux matériaux sans texture, dans [0, 1] : plus elle est
    /// haute, plus ils l'emportent sur leurs voisins texturés au mélange par hauteur.
    pub flat_material_height: f32,
    /// Amplitude (voxels) du décalage des lectures de texture (domain warping), qui
    /// casse l'alignement des copies d'une texture répétée. 0 = désactivé.
    pub warp_amplitude: f32,
    /// Longueur d'onde (voxels) du warp. Étirement de la texture ≲ 6·A/λ au pire :
    /// garder λ ≳ 25·A, sinon la texture paraît coulée.
    pub warp_wavelength: f32,
    /// Longueur d'onde (voxels) de la variation de couleur à grande échelle.
    pub tint_wavelength: f32,
    /// Variation relative de luminosité (± fraction), dans [0, 1[.
    pub brightness_variation: f32,
    /// Variation de teinte chaud/froid (± fraction sur rouge et bleu), dans [0, 1[.
    pub hue_variation: f32,
}

impl Default for MaterialTexturesConfig {
    fn default() -> Self {
        MaterialTexturesConfig {
            triplanar_sharpness: 4.0,
            projection_blend_depth: 0.2,
            material_blend_depth: 0.2,
            normal_strength: 1.0,
            ao_strength: 1.0,
            flat_material_height: 0.5,
            warp_amplitude: 6.0,
            warp_wavelength: 150.0,
            tint_wavelength: 300.0,
            brightness_variation: 0.15,
            hue_variation: 0.05,
        }
    }
}

impl ConfigFile for MaterialTexturesConfig {
    const FILE_NAME: &'static str = "material_textures.ron";

    fn validate(&self) -> Result<()> {
        ensure!(
            self.triplanar_sharpness > 0.0,
            "triplanar_sharpness doit être > 0"
        );
        // Profondeur nulle : facteurs tous nuls au pixel où deux scores sont égaux.
        ensure!(
            self.projection_blend_depth > 0.0 && self.material_blend_depth > 0.0,
            "projection_blend_depth et material_blend_depth doivent être > 0"
        );
        ensure!(self.normal_strength >= 0.0, "normal_strength doit être ≥ 0");
        ensure!(
            (0.0..=1.0).contains(&self.ao_strength),
            "ao_strength doit être dans [0, 1]"
        );
        ensure!(
            (0.0..1.0).contains(&self.flat_material_height),
            "flat_material_height doit être dans [0, 1]"
        );
        ensure!(self.warp_amplitude >= 0.0, "warp_amplitude doit être ≥ 0");
        ensure!(
            self.warp_wavelength > 0.0 && self.tint_wavelength > 0.0,
            "warp_wavelength et tint_wavelength doivent être > 0"
        );
        ensure!(
            (0.0..1.0).contains(&self.brightness_variation)
                && (0.0..1.0).contains(&self.hue_variation),
            "brightness_variation et hue_variation doivent être dans [0, 1["
        );
        Ok(())
    }
}
