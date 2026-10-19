//! Mesure le coût du maillage par LOD et sa répartition entre postes (normales,
//! matériaux, relief…), sans lancer le moteur.
//!
//! `cargo run --release -p Dromon-engine --features profiling --example mesh_bench [-- N]`
//! N = chunks mesurés par LOD (200 par défaut).

use anyhow::Result;

fn main() -> Result<()> {
    let per_lod = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(200);
    print!("{}", dromon_engine::mesh_bench(per_lod)?);
    Ok(())
}
