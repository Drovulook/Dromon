//! **Texgen** : textures générées par compute shader depuis une recette Slang de l'app
//! (`<app>/dev_res/texgen/<nom>.slang`, harnais `dev_res/shaders/procgen/texgen.slang`).
//! [`run`] : compilation (`slangc`) → 2 passes GPU → PNG dans `work/texgen/<nom>/`.

mod compile;
mod export;
mod gpu;

use crate::AppDirs;
use crate::app::engine::rendering_context::RenderingContext;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;

/// Bornes de `size` : puissance de 2 (chaîne de mipmaps complète une fois dans le
/// texture array du terrain).
const MIN_SIZE: u32 = 64;
const MAX_SIZE: u32 = 4096;

/// Génère les cartes de `recipe` (`size × size`, graine `seed`). Bloquant (compilation
/// + GPU + PNG). Rend un résumé JSON : dossier, plage de hauteur brute, durées.
pub(super) fn run(
    context: &Arc<RenderingContext>,
    recipe: &str,
    size: u32,
    seed: u32,
) -> Result<Value> {
    // Le nom devient un chemin : rien qui puisse sortir de dev_res/texgen/.
    ensure!(
        !recipe.is_empty()
            && recipe
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'),
        "nom de recette invalide : `{recipe}` (attendu : [a-z0-9_-])"
    );
    ensure!(
        size.is_power_of_two() && (MIN_SIZE..=MAX_SIZE).contains(&size),
        "taille {size} invalide : puissance de 2 entre {MIN_SIZE} et {MAX_SIZE}"
    );

    let start = Instant::now();
    let shaders = compile::compile(recipe, &context.logger)?;
    let compiled = Instant::now();
    let maps = gpu::generate(context, &shaders, size, seed)?;
    let generated = Instant::now();
    let dir = AppDirs::get().work().join("texgen").join(recipe);
    let range = export::export(&maps, &dir)?;
    let exported = Instant::now();

    let ms = |from: Instant, to: Instant| (to - from).as_millis() as u64;
    let (compile_ms, gpu_ms, png_ms) = (
        ms(start, compiled),
        ms(compiled, generated),
        ms(generated, exported),
    );
    context.logger.info(&format!(
        "texgen `{recipe}` ({size}², graine {seed}) → {} : hauteur [{:.4}, {:.4}] tuile ; \
         slangc {compile_ms} ms, GPU {gpu_ms} ms, PNG {png_ms} ms",
        dir.display(),
        range.min,
        range.max,
    ));
    Ok(json!({
        "dir": dir,
        "size": size,
        "seed": seed,
        "height_min": range.min,
        "height_max": range.max,
        "ms": { "compile": compile_ms, "gpu": gpu_ms, "png": png_ms },
    }))
}
