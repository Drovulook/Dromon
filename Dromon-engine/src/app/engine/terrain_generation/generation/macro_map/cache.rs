//! **Cache disque** de la carte macro, pour ne pas la recalculer à chaque lancement.
//! Le fichier porte la clé de la config qui l'a produite : une config modifiée
//! l'invalide. Ordre des octets natif : cache local, pas un format d'échange.

use std::fs::File;
use std::io::{BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::Path;

use anyhow::{Context, Result, ensure};

use super::{GridShape, MacroGrid, MacroMap};
use crate::app::engine::terrain_generation::CHUNK_SIZE;
use crate::config::{WorldGenConfig, WorldMacroConfig};

/// Emplacement par défaut, à côté des PNG (`cargo clean` l'efface : il sera recalculé).
pub const MACRO_MAP_CACHE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../target/world_maps/macro_map.bin");

const MAGIC: &[u8; 8] = b"DRMACRO\0";
/// À incrémenter quand l'algorithme change sans que la config change : la clé ne
/// voit que la config, pas le code.
const VERSION: u32 = 1;

/// Identité d'une carte : la config macro entière en RON (pas un hash : `f64`
/// n'implémente pas `Hash`, et `DefaultHasher` n'est pas stable entre versions de Rust).
pub(super) fn cache_key(config: &WorldMacroConfig) -> String {
    let ron = ron::to_string(config).expect("config macro sérialisable");
    format!("v{VERSION} chunk={CHUNK_SIZE} {ron}")
}

impl MacroMap {
    /// Écrit la carte dans `path` (dossier créé au besoin).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("création de {}", dir.display()))?;
        }
        let file = File::create(path).with_context(|| format!("création de {}", path.display()))?;
        let mut w = BufWriter::new(file);
        w.write_all(MAGIC)?;
        w.write_all(&(self.cache_key.len() as u64).to_ne_bytes())?;
        w.write_all(self.cache_key.as_bytes())?;
        w.write_all(&self.radius.to_ne_bytes())?;
        w.write_all(&self.sea_level.to_ne_bytes())?;
        w.write_all(&self.max_height.to_ne_bytes())?;
        w.write_all(&(self.inland_seas_filled as u64).to_ne_bytes())?;
        w.write_all(&(self.islands_removed as u64).to_ne_bytes())?;
        // Même ordre que dans `load`.
        for grid in [
            &self.continentality,
            &self.coast_distance,
            &self.mountains,
            &self.relief,
            &self.climate_relief,
            &self.climate_coast,
            &self.latitude_temperature,
            &self.temperature,
            &self.wind_east,
            &self.wind_north,
            &self.current_east,
            &self.current_north,
            &self.upwelling,
            &self.sst,
            &self.precipitation,
        ] {
            write_grid(&mut w, grid)?;
        }
        w.flush().with_context(|| format!("écriture de {}", path.display()))
    }

    /// Relit la carte de `path`. `Ok(None)` : fichier absent ou produit par une autre
    /// config (ou une autre `VERSION`).
    pub fn load(config: &WorldGenConfig, path: &Path) -> Result<Option<MacroMap>> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("ouverture de {}", path.display())),
        };
        let mut r = BufReader::new(file);
        let read = |r: &mut BufReader<File>| -> Result<Option<MacroMap>> {
            let mut magic = [0; 8];
            r.read_exact(&mut magic)?;
            ensure!(&magic == MAGIC, "pas un cache de carte macro");
            let key_len = read_u64(r)? as usize;
            ensure!(key_len <= 1 << 20, "clé de cache corrompue");
            let mut key = vec![0; key_len];
            r.read_exact(&mut key)?;
            let cache_key = cache_key(&config.macro_);
            if key != cache_key.as_bytes() {
                return Ok(None);
            }
            Ok(Some(MacroMap {
                cache_key,
                shape: GridShape { n: 0, cell_size: 0.0, origin: 0.0 }, // fixée plus bas
                radius: read_f64(r)?,
                sea_level: read_f32(r)?,
                max_height: read_f32(r)?,
                inland_seas_filled: read_u64(r)? as usize,
                islands_removed: read_u64(r)? as usize,
                // Les champs d'un littéral sont évalués dans l'ordre écrit : même
                // ordre que dans `save`.
                continentality: read_grid(r)?,
                coast_distance: read_grid(r)?,
                mountains: read_grid(r)?,
                relief: read_grid(r)?,
                climate_relief: read_grid(r)?,
                climate_coast: read_grid(r)?,
                latitude_temperature: read_grid(r)?,
                temperature: read_grid(r)?,
                wind_east: read_grid(r)?,
                wind_north: read_grid(r)?,
                current_east: read_grid(r)?,
                current_north: read_grid(r)?,
                upwelling: read_grid(r)?,
                sst: read_grid(r)?,
                precipitation: read_grid(r)?,
            }))
        };
        let mut map = read(&mut r).with_context(|| format!("lecture de {}", path.display()))?;
        if let Some(map) = &mut map {
            map.shape = map.continentality.shape();
        }
        Ok(map)
    }

    /// Relit le cache s'il correspond à `config`, sinon calcule la carte et l'écrit.
    /// Un cache illisible ou impossible à écrire n'est pas fatal : on recalcule.
    pub fn load_or_build(config: &WorldGenConfig, path: &Path) -> MacroMap {
        match MacroMap::load(config, path) {
            Ok(Some(map)) => return map,
            Ok(None) => {}
            Err(e) => eprintln!("Cache de carte macro ignoré : {e:#}"),
        }
        let map = MacroMap::build(config);
        if let Err(e) = map.save(path) {
            eprintln!("Cache de carte macro non écrit : {e:#}");
        }
        map
    }
}

fn write_grid(w: &mut impl Write, grid: &MacroGrid) -> Result<()> {
    let shape = grid.shape();
    w.write_all(&(shape.n as u64).to_ne_bytes())?;
    w.write_all(&shape.cell_size.to_ne_bytes())?;
    w.write_all(&shape.origin.to_ne_bytes())?;
    w.write_all(bytemuck::cast_slice(grid.values()))?;
    Ok(())
}

fn read_grid(r: &mut impl Read) -> Result<MacroGrid> {
    let n = read_u64(r)? as usize;
    // Garde-fou : un `n` corrompu ne doit pas allouer des téraoctets.
    ensure!(n <= 1 << 16, "taille de grille corrompue ({n})");
    let shape = GridShape {
        n,
        cell_size: read_f64(r)?,
        origin: read_f64(r)?,
    };
    // Lecture directe dans le `Vec<f32>` (vu comme octets) : aucune copie.
    let mut data = vec![0.0f32; n * n];
    r.read_exact(bytemuck::cast_slice_mut(&mut data))?;
    Ok(MacroGrid::from_vec(shape, data))
}

fn read_u64(r: &mut impl Read) -> Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_ne_bytes(b))
}

fn read_f64(r: &mut impl Read) -> Result<f64> {
    let mut b = [0; 8];
    r.read_exact(&mut b)?;
    Ok(f64::from_ne_bytes(b))
}

fn read_f32(r: &mut impl Read) -> Result<f32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(f32::from_ne_bytes(b))
}
