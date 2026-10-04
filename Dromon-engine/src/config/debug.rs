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
}

impl Default for DebugConfig {
    fn default() -> Self {
        DebugConfig {
            log_batch_stats: true,
            log_gpu_usage: true,
            mesh: MeshDebug::default(),
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
