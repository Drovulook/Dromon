//! Mesure le coût du maillage par LOD et sa répartition entre postes (normales,
//! matériaux, relief…), sans lancer le moteur.
//!
//! `cargo run --release -p Dromon-engine --features profiling --example mesh_bench [-- N]`
//! N = chunks mesurés par LOD (200 par défaut). Monde de Physis.

use anyhow::Result;
use dromon_engine::AppDirs;

fn main() -> Result<()> {
    AppDirs::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../apps/Physis")).install()?;
    let per_lod = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(200);
    print!("{}", dromon_engine::mesh_bench(per_lod)?);
    Ok(())
}
