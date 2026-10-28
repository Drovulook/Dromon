//! **Banc de mesure du maillage** (`examples/mesh_bench.rs`) : coût de `mesh_chunk` par
//! LOD, et sa répartition entre les postes instrumentés par `profile!`.
//!
//! Mono-thread, sur le **même** échantillon de chunks terrestres pour tous les LOD : les
//! niveaux se comparent entre eux, et les temps ne souffrent pas du partage des cœurs
//! (hyperthreading) qui gonfle les mesures en jeu.

use std::{fmt::Write, path::Path, sync::Arc, time::Instant};

use anyhow::Result;
use glam::IVec2;

use super::{
    CHUNK_SIZE, ChunkStore, LodGrid, MACRO_MAP_CACHE, MacroMap, TerrainSnapshot, TerrainSource,
    mesh_chunk,
};
use crate::config::{
    ConfigFile, MaterialRulesConfig, MaterialsConfig, MeshDebug, RenderConfig, WorldGenConfig,
};
use crate::profiling;

/// Maille `per_lod` chunks à chaque LOD de `render.ron` et rend le rapport en texte.
pub fn mesh_bench(per_lod: usize) -> Result<String> {
    let config = WorldGenConfig::load()?;
    let materials = MaterialsConfig::load()?;
    let rules = MaterialRulesConfig::load()?;
    let max_lod = RenderConfig::load()?.terrain.lod.max_lod();
    let (macro_map, _) = MacroMap::load_or_build(&config, Path::new(MACRO_MAP_CACHE))?;
    let source = Arc::new(TerrainSource::new(&config, &materials, &rules, macro_map)?);
    let snapshot = TerrainSnapshot::new(&source, &ChunkStore::default(), MeshDebug::default());

    let chunks = land_sample(&source, config.macro_.world.sea_level as f32, per_lod);
    let n = chunks.len().max(1);
    let mut out = String::new();
    writeln!(out, "{} chunks terrestres par LOD, mono-thread", chunks.len())?;

    profiling::set_enabled(true);
    for lod in 0..=max_lod {
        // Chaque chunk seul dans sa grille : aucun voisin, donc aucune transition.
        let grids: Vec<LodGrid> = chunks.iter().map(|&c| single(c, lod)).collect();

        // Temps nu d'abord : c'est le chiffre de référence, le profilage ajoutant
        // ~100 ns par bloc mesuré.
        profiling::set_recording(false);
        let start = Instant::now();
        let mut vertices = 0;
        for (&c, grid) in chunks.iter().zip(&grids) {
            vertices += mesh_chunk(&snapshot, grid, c).vertices.len();
        }
        let bare = start.elapsed();

        // Même travail, profilé : la répartition entre postes.
        profiling::set_recording(true);
        for (&c, grid) in chunks.iter().zip(&grids) {
            mesh_chunk(&snapshot, grid, c);
        }
        profiling::set_recording(false);

        writeln!(
            out,
            "\n── LOD {lod} (pas {}) : {} µs/chunk sans profilage, {} sommets/chunk",
            1 << lod,
            bare.as_micros() / n as u128,
            vertices / n,
        )?;
        write_tree(&mut out, &profiling::serialize_and_clear(), n)?;
    }
    Ok(out)
}

/// `n` chunks terrestres répartis sur tout le monde (plaines comme montagnes).
fn land_sample(source: &TerrainSource, sea_level: f32, n: usize) -> Vec<IVec2> {
    let size = CHUNK_SIZE as f32;
    let land: Vec<IVec2> = source
        .world()
        .coords()
        .into_iter()
        .filter(|c| {
            let center = c.as_vec2() * size + size / 2.0;
            source.terrain_height(center.x, center.y) > sea_level
        })
        .collect();
    // Pris à intervalle régulier dans la liste plutôt que groupés : un seul massif
    // fausserait la moyenne.
    let stride = (land.len() / n.max(1)).max(1);
    land.into_iter().step_by(stride).take(n).collect()
}

/// Grille réduite au seul chunk `c`, au niveau `lod`.
fn single(c: IVec2, lod: u8) -> LodGrid {
    let mut grid = LodGrid::new(vec![c]);
    grid.set_raw_lods(|_, _| lod);
    grid.rebalance();
    grid
}

/// Arbre de profilage (format de `profiling::serialize_and_clear`, parcours en
/// profondeur) en tableau : temps total et **propre** (hors enfants) par chunk, appels
/// par chunk.
fn write_tree(out: &mut String, payload: &str, n: usize) -> Result<()> {
    struct Row<'a> {
        depth: usize,
        ns: u128,
        count: u64,
        name: &'a str,
    }
    let rows: Vec<Row> = payload
        .split('\u{1e}')
        .filter(|r| !r.is_empty())
        .filter_map(|r| {
            let mut f = r.split('\u{1f}');
            Some(Row {
                depth: f.next()?.parse().ok()?,
                ns: f.next()?.parse().ok()?,
                count: f.next()?.parse().ok()?,
                // Les noms automatiques sont des chemins complets : garder la fin.
                name: f.next()?.rsplit("::").next()?,
            })
        })
        .collect();

    writeln!(
        out,
        "  {:<34} {:>10} {:>10} {:>12}",
        "poste", "µs/chunk", "propre", "appels/chunk"
    )?;
    let per_chunk = |ns: u128| ns as f64 / n as f64 / 1000.0;
    for (i, row) in rows.iter().enumerate() {
        // Enfants directs : les lignes suivantes plus profondes d'un cran, jusqu'au
        // retour à la profondeur de `row`.
        let children: u128 = rows[i + 1..]
            .iter()
            .take_while(|r| r.depth > row.depth)
            .filter(|r| r.depth == row.depth + 1)
            .map(|r| r.ns)
            .sum();
        let label = format!("{}{}", "  ".repeat(row.depth), row.name);
        writeln!(
            out,
            "  {:<34} {:>10.0} {:>10.0} {:>12.1}",
            label,
            per_chunk(row.ns),
            per_chunk(row.ns.saturating_sub(children)),
            row.count as f64 / n as f64,
        )?;
    }
    Ok(())
}
