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
    let (lo, hi) = map.temperature.min_max();
    let n = map.temperature.shape().n;
    println!("Climat : grille {n} × {n} ; température {lo:.1} → {hi:.1} °C");
    // Anomalie de SST (courants) : en mer seulement.
    let (mut cold, mut warm) = (0.0f32, 0.0f32);
    for ((&sst, &eq), &d) in map
        .sst
        .values()
        .iter()
        .zip(map.latitude_temperature.values())
        .zip(map.climate_coast.values())
    {
        if d < 0.0 {
            cold = cold.min(sst - eq);
            warm = warm.max(sst - eq);
        }
    }
    println!("Anomalie de SST (courants) : {cold:+.1} → {warm:+.1} °C");
    // Précipitations sur terre : déciles, pour régler la part de déserts.
    let mut land_rain: Vec<f32> = map
        .precipitation
        .values()
        .iter()
        .zip(map.climate_coast.values())
        .filter(|&(_, &d)| d > 0.0)
        .map(|(&p, _)| p)
        .collect();
    land_rain.sort_by(f32::total_cmp);
    if !land_rain.is_empty() {
        let at = |q: f32| land_rain[((land_rain.len() - 1) as f32 * q) as usize];
        let dry = land_rain.iter().filter(|&&p| p < 0.25).count() as f32 / land_rain.len() as f32;
        println!(
            "Précipitations (terre) : min {:.2}, médiane {:.2}, max {:.2} ; {:.0} % sous 0,25 (sec)",
            at(0.0),
            at(0.5),
            at(1.0),
            dry * 100.0
        );
    }

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/world_maps");
    for path in map.export_pngs(&dir)? {
        println!("→ {}", path.display());
    }
    Ok(())
}
