//! Hot reload des configs : celles du moteur, et celles de la génération du monde quand
//! la scène a créé un terrain. Chaque champ de chaque config est associé à **un** type
//! de rechargement ([`Reload`]) ; les fonctions `*_reloads` le font par déstructuration
//! exhaustive (sans `..`) : ajouter un champ à une config est une erreur de compilation
//! ici tant qu'on ne l'a pas classé.

use crate::config::{
    ConfigFile, DebugConfig, EngineConfig, EnvironmentConfig, MaterialRulesConfig,
    MaterialTexturesConfig, MaterialsConfig, RenderConfig, ShadowSettings, WorldDetailConfig,
    WorldGenConfig, WorldMacroConfig,
};
use anyhow::Result;
use std::collections::BTreeMap;

/// Ce qu'il faut refaire quand un champ change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Reload {
    /// Lu à chaque frame (UBO, état CPU du monde : caméra, horloge…) : rien à recréer.
    Live,
    Swapchain,
    /// Pipelines des meshes (objets, terrain).
    MeshPipelines,
    /// Sampler des textures, et les descriptor sets qui le référencent.
    Sampler,
    ShadowMap,
    /// Régénération du terrain sur la même carte macro (et de l'eau, créée avec lui).
    Terrain,
    /// Nouvelle carte macro, puis régénération du terrain.
    MacroMap,
    /// Table GPU des matériaux et IDs de la génération.
    Materials,
}

impl Reload {
    /// Le traitement de ce rechargement est-il écrit (cf. `Renderer::hot_reload`) ?
    fn is_supported(self) -> bool {
        match self {
            Reload::Live
            | Reload::Swapchain
            | Reload::MeshPipelines
            | Reload::Sampler
            | Reload::ShadowMap
            | Reload::Terrain => true,
            Reload::MacroMap | Reload::Materials => false,
        }
    }
}

/// Rechargements à faire, avec les champs modifiés qui les demandent (pour les logs).
#[derive(Default)]
pub(crate) struct Reloads(BTreeMap<Reload, Vec<&'static str>>);

impl Reloads {
    pub(crate) fn contains(&self, reload: Reload) -> bool {
        self.0.contains_key(&reload)
    }

    fn check<T: PartialEq>(&mut self, field: &'static str, old: &T, new: &T, reload: Reload) {
        if old != new {
            self.0.entry(reload).or_default().push(field);
        }
    }

    /// Ex. `"Terrain (terrain, water)"`, une entrée par rechargement non géré.
    fn unsupported(&self) -> Vec<String> {
        self.0
            .iter()
            .filter(|(reload, _)| !reload.is_supported())
            .map(|(reload, fields)| format!("{reload:?} ({})", fields.join(", ")))
            .collect()
    }
}

/// Configs rechargées : celle du moteur, et celle de la génération du monde s'il y a un
/// terrain.
pub(crate) struct Configs {
    pub engine: EngineConfig,
    pub world_gen: Option<WorldGenConfig>,
}

pub(crate) enum ConfigReload {
    /// Fichier non géré : inconnu, ou config de génération alors qu'il n'y a pas de
    /// terrain.
    Unhandled,
    /// Contenu identique : sauvegarde sans modification.
    Unchanged,
    /// Nouvelles configs et rechargements à faire, tous gérés.
    Apply(Box<Configs>, Reloads),
    /// Rechargements pas encore gérés : rien n'est appliqué, pour ne pas laisser le
    /// moteur à moitié à jour.
    Unsupported(Vec<String>),
}

/// Relit `file_name` et le compare aux configs en place. `world_gen` : celle du terrain,
/// `None` sans terrain. `Err` : fichier illisible ou invalide.
pub(crate) fn reload(
    engine: &EngineConfig,
    world_gen: Option<&WorldGenConfig>,
    file_name: &str,
) -> Result<ConfigReload> {
    let mut next = Configs {
        engine: engine.clone(),
        world_gen: world_gen.cloned(),
    };
    let mut reloads = Reloads::default();
    match file_name {
        EnvironmentConfig::FILE_NAME => {
            next.engine.environment = EnvironmentConfig::load()?;
            environment_reloads(&engine.environment, &next.engine.environment, &mut reloads);
        }
        RenderConfig::FILE_NAME => {
            next.engine.render = RenderConfig::load()?;
            render_reloads(&engine.render, &next.engine.render, &mut reloads);
        }
        DebugConfig::FILE_NAME => {
            next.engine.debug = DebugConfig::load()?;
            debug_reloads(&engine.debug, &next.engine.debug, &mut reloads);
        }
        MaterialsConfig::FILE_NAME => {
            next.engine.materials = MaterialsConfig::load()?;
            let MaterialsConfig { materials } = &engine.materials;
            let new = &next.engine.materials.materials;
            reloads.check("materials", materials, new, Reload::Materials);
        }
        MaterialRulesConfig::FILE_NAME => {
            next.engine.material_rules = MaterialRulesConfig::load()?;
            let new = &next.engine.material_rules;
            material_rules_reloads(&engine.material_rules, new, &mut reloads);
        }
        MaterialTexturesConfig::FILE_NAME => {
            next.engine.material_textures = MaterialTexturesConfig::load()?;
            let new = &next.engine.material_textures;
            material_textures_reloads(&engine.material_textures, new, &mut reloads);
        }
        // Configs de génération : relues aux chemins par défaut, ceux de
        // `WorldGenConfig::load` (la scène peut en principe en passer d'autres).
        WorldDetailConfig::FILE_NAME => {
            let (Some(old), Some(new)) = (world_gen, next.world_gen.as_mut()) else {
                return Ok(ConfigReload::Unhandled);
            };
            new.detail = WorldDetailConfig::load()?;
            detail_reloads(&old.detail, &new.detail, &mut reloads);
        }
        WorldMacroConfig::FILE_NAME => {
            let (Some(old), Some(new)) = (world_gen, next.world_gen.as_mut()) else {
                return Ok(ConfigReload::Unhandled);
            };
            new.macro_ = WorldMacroConfig::load()?;
            macro_reloads(&old.macro_, &new.macro_, &mut reloads);
        }
        _ => return Ok(ConfigReload::Unhandled),
    }

    let unsupported = reloads.unsupported();
    Ok(if reloads.0.is_empty() {
        ConfigReload::Unchanged
    } else if unsupported.is_empty() {
        ConfigReload::Apply(Box::new(next), reloads)
    } else {
        ConfigReload::Unsupported(unsupported)
    })
}

fn detail_reloads(old: &WorldDetailConfig, new: &WorldDetailConfig, out: &mut Reloads) {
    let WorldDetailConfig {
        mountain,
        hills,
        hills_coast_fade,
    } = old;
    out.check("mountain", mountain, &new.mountain, Reload::Terrain);
    out.check("hills", hills, &new.hills, Reload::Terrain);
    out.check("hills_coast_fade", hills_coast_fade, &new.hills_coast_fade, Reload::Terrain);
}

fn macro_reloads(old: &WorldMacroConfig, new: &WorldMacroConfig, out: &mut Reloads) {
    let WorldMacroConfig {
        seed,
        world,
        cell_size,
        continents,
        coast,
        mountains,
        climate,
    } = old;
    out.check("seed", seed, &new.seed, Reload::MacroMap);
    out.check("world", world, &new.world, Reload::MacroMap);
    out.check("cell_size", cell_size, &new.cell_size, Reload::MacroMap);
    out.check("continents", continents, &new.continents, Reload::MacroMap);
    out.check("coast", coast, &new.coast, Reload::MacroMap);
    out.check("mountains", mountains, &new.mountains, Reload::MacroMap);
    out.check("climate", climate, &new.climate, Reload::MacroMap);
}

fn environment_reloads(old: &EnvironmentConfig, new: &EnvironmentConfig, out: &mut Reloads) {
    let EnvironmentConfig {
        latitude_deg,
        clock,
        sun,
        day_night,
        atmosphere,
        stars,
        water,
    } = old;
    out.check("latitude_deg", latitude_deg, &new.latitude_deg, Reload::Live);
    out.check("clock", clock, &new.clock, Reload::Live);
    out.check("sun", sun, &new.sun, Reload::Live);
    out.check("day_night", day_night, &new.day_night, Reload::Live);
    out.check("atmosphere", atmosphere, &new.atmosphere, Reload::Live);
    out.check("stars", stars, &new.stars, Reload::Live);
    out.check("water", water, &new.water, Reload::Live);
}

fn render_reloads(old: &RenderConfig, new: &RenderConfig, out: &mut Reloads) {
    let RenderConfig {
        present_mode,
        display_mode,
        camera,
        shadow,
        terrain,
        water,
        tonemap,
        textures,
    } = old;
    out.check("present_mode", present_mode, &new.present_mode, Reload::Swapchain);
    out.check("display_mode", display_mode, &new.display_mode, Reload::MeshPipelines);
    out.check("camera", camera, &new.camera, Reload::Live);
    out.check("terrain", terrain, &new.terrain, Reload::Terrain);
    // Quadtree de l'eau, créée avec le terrain.
    out.check("water", water, &new.water, Reload::Terrain);
    out.check("tonemap", tonemap, &new.tonemap, Reload::Live);
    out.check("textures", textures, &new.textures, Reload::Sampler);

    let ShadowSettings {
        map_resolution,
        normal_offset_texels,
        depth_bias_texels,
        static_scene,
        terrain: terrain_box,
    } = shadow;
    let new_shadow = &new.shadow;
    out.check(
        "shadow.map_resolution",
        map_resolution,
        &new_shadow.map_resolution,
        Reload::ShadowMap,
    );
    out.check(
        "shadow.normal_offset_texels",
        normal_offset_texels,
        &new_shadow.normal_offset_texels,
        Reload::Live,
    );
    out.check(
        "shadow.depth_bias_texels",
        depth_bias_texels,
        &new_shadow.depth_bias_texels,
        Reload::Live,
    );
    out.check("shadow.static_scene", static_scene, &new_shadow.static_scene, Reload::Live);
    out.check("shadow.terrain", terrain_box, &new_shadow.terrain, Reload::Live);
}

fn debug_reloads(old: &DebugConfig, new: &DebugConfig, out: &mut Reloads) {
    let DebugConfig {
        log_batch_stats,
        log_gpu_usage,
        shadows,
        mesh,
    } = old;
    out.check("log_batch_stats", log_batch_stats, &new.log_batch_stats, Reload::Live);
    out.check("log_gpu_usage", log_gpu_usage, &new.log_gpu_usage, Reload::Live);
    out.check("shadows", shadows, &new.shadows, Reload::Live);
    out.check("mesh", mesh, &new.mesh, Reload::Terrain);
}

fn material_rules_reloads(old: &MaterialRulesConfig, new: &MaterialRulesConfig, out: &mut Reloads) {
    let MaterialRulesConfig {
        strata,
        snow_edge,
        snow_dryness,
        desert_edge,
        sandstone_edge,
        outcrop,
        shore,
        dirt_patches,
        slope,
        noise,
        coast_moisture,
    } = old;
    // Tout sauf `coast_moisture` (UBO) sert au placement des matériaux par la génération.
    out.check("strata", strata, &new.strata, Reload::Terrain);
    out.check("snow_edge", snow_edge, &new.snow_edge, Reload::Terrain);
    out.check("snow_dryness", snow_dryness, &new.snow_dryness, Reload::Terrain);
    out.check("desert_edge", desert_edge, &new.desert_edge, Reload::Terrain);
    out.check("sandstone_edge", sandstone_edge, &new.sandstone_edge, Reload::Terrain);
    out.check("outcrop", outcrop, &new.outcrop, Reload::Terrain);
    out.check("shore", shore, &new.shore, Reload::Terrain);
    out.check("dirt_patches", dirt_patches, &new.dirt_patches, Reload::Terrain);
    out.check("slope", slope, &new.slope, Reload::Terrain);
    out.check("noise", noise, &new.noise, Reload::Terrain);
    out.check("coast_moisture", coast_moisture, &new.coast_moisture, Reload::Live);
}

fn material_textures_reloads(
    old: &MaterialTexturesConfig,
    new: &MaterialTexturesConfig,
    out: &mut Reloads,
) {
    // Tout part dans l'UBO ; déstructuré quand même pour forcer le classement des
    // champs futurs.
    let MaterialTexturesConfig {
        triplanar_sharpness: _,
        projection_blend_depth: _,
        material_blend_depth: _,
        normal_strength: _,
        ao_strength: _,
        flat_material_height: _,
        warp_amplitude: _,
        warp_wavelength: _,
        tint_wavelength: _,
        brightness_variation: _,
        hue_variation: _,
    } = old;
    out.check("material_textures", old, new, Reload::Live);
}
