//! Calcule la carte macro et l'exporte en PNG, sans lancer le moteur.
//!
//! `cargo run --release -p Dromon-engine --example macro_map`
//! Sortie : `target/world_maps/`.

use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use dromon_engine::MacroMap;
use dromon_engine::config::WorldGenConfig;

fn main() -> Result<()> {
    let config = WorldGenConfig::load()?;

    let start = Instant::now();
    let map = MacroMap::build(&config);
    let shape = map.shape();
    println!(
        "Carte macro {n} × {n} (cellules de {c} voxels) en {t:.2?}",
        n = shape.n,
        c = shape.cell_size,
        t = start.elapsed()
    );

    let (lo, hi) = map.coast_distance.min_max();
    println!(
        "Terre : {:.0} % du disque ; distance à la côte : {lo:.0} (large) → {hi:.0} (intérieur) voxels",
        map.land_fraction() * 100.0
    );
    println!(
        "Mers intérieures comblées : {} ; petites îles supprimées : {}",
        map.inland_seas_filled, map.islands_removed
    );
    let (lo, hi) = map.relief.min_max();
    println!(
        "Relief macro : {lo:.0} → {hi:.0} voxels (mer à {}, plafond {})",
        config.macro_.world.sea_level, config.macro_.world.max_height
    );

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/world_maps");
    for path in map.export_pngs(&dir)? {
        println!("→ {}", path.display());
    }
    Ok(())
}
