//! `debug.ron` : traces et aides visuelles de développement.

use serde::{Deserialize, Serialize};

use super::ConfigFile;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DebugConfig {
    /// Une ligne par lot de re-maillage : chunks installés, part du cache, coût par LOD.
    /// Seule façon de savoir si le cache est rentable pour un style de déplacement.
    pub log_batch_stats: bool,
    /// Occupation et fragmentation de l'allocateur GPU, à chaque lot installé.
    pub log_gpu_usage: bool,
    pub mesh: MeshDebug,
    pub water: WaterDebug,
}

impl Default for DebugConfig {
    fn default() -> Self {
        DebugConfig {
            log_batch_stats: true,
            log_gpu_usage: true,
            mesh: MeshDebug::default(),
            water: WaterDebug::default(),
        }
    }
}

impl ConfigFile for DebugConfig {
    const FILE_NAME: &'static str = "debug.ron";
}

/// Options du mailleur de terrain.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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

/// Aides visuelles de la mer (quadtree CDLOD, geomorphing).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaterDebug {
    /// Une couleur par niveau de nœud.
    pub lod_color: bool,
    /// Assombrit les sommets en cours de geomorphing (noir à `k = 1`).
    pub morph_color: bool,
    /// Amplitude de vagues sinusoïdales de test (0 = plat). Font apparaître les
    /// fissures entre niveaux si le geomorphing est faux.
    pub test_wave_amplitude: f32,
    /// Longueur d'onde de ces vagues.
    pub test_wave_length: f32,
}
