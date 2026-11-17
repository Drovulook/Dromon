//! `debug.ron` : traces et aides visuelles de développement.

use serde::{Deserialize, Serialize};

use super::ConfigFile;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DebugConfig {
    /// Bilan du re-maillage chaque seconde : chunks installés, part du cache, coût par LOD.
    /// Seule façon de savoir si le cache est rentable pour un style de déplacement.
    pub log_batch_stats: bool,
    /// Occupation et fragmentation de l'allocateur GPU, avec le bilan du re-maillage.
    pub log_gpu_usage: bool,
    /// `false` : tout est éclairé (la passe d'ombre tourne quand même, seul son résultat
    /// est ignoré).
    pub shadows: bool,
    /// `true` : la caméra peut sortir du disque du monde (vue d'ensemble, bord du monde).
    pub camera_unbounded: bool,
    pub mesh: MeshDebug,
    pub lookdev: LookdevParams,
}

impl Default for DebugConfig {
    fn default() -> Self {
        DebugConfig {
            log_batch_stats: true,
            log_gpu_usage: true,
            shadows: true,
            camera_unbounded: false,
            mesh: MeshDebug::default(),
            lookdev: LookdevParams::default(),
        }
    }
}

/// Mode lookdev : pas de terrain, un sol plat à la place, pour examiner des assets.
/// Rechargé à chaud (entrée, sortie, taille du sol). Sans effet sur une scène sans terrain.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LookdevParams {
    /// `true` : `generate_terrain` pose le sol au lieu de créer le terrain.
    pub enabled: bool,
    /// Côté du sol, centré sur l'origine (unités monde).
    pub ground_size: f32,
}

impl Default for LookdevParams {
    fn default() -> Self {
        LookdevParams {
            enabled: false,
            ground_size: 50.0,
        }
    }
}

impl ConfigFile for DebugConfig {
    const FILE_NAME: &'static str = "debug.ron";
}

/// Options du mailleur de terrain.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MeshDebug {
    /// Parois verticales au bord du monde.
    pub world_walls: bool,
    /// Face du dessous de chaque chunk (au plancher du monde).
    pub world_bottom: bool,
    /// Teinte en magenta les cellules de transition (Transvoxel), pour voir où elles se
    /// posent le long des frontières de LOD.
    pub transition_color: bool,
}

impl Default for MeshDebug {
    fn default() -> Self {
        MeshDebug {
            world_walls: true,
            world_bottom: true,
            transition_color: false,
        }
    }
}
