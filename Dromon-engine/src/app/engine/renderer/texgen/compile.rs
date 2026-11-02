//! Recette → SPIR-V des deux passes, par `slangc` : un processus par point d'entrée,
//! lancés en parallèle. Les `.spv` restent dans `work/texgen/<recette>/` (inspection).

use crate::AppDirs;
use crate::app::logger::Logger;
use anyhow::{Context, Result, bail, ensure};
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// Racine des includes des recettes (`procgen/texgen.slang`). `env!` est développé à la
/// compilation du moteur : c'est bien `Dromon-engine/`, quelle que soit l'app.
const ENGINE_SHADERS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/dev_res/shaders");

/// SPIR-V (mots de 32 bits) des deux passes d'une recette.
pub(super) struct RecipeShaders {
    /// `surfaceMain` : hauteur + albédo.
    pub surface: Vec<u32>,
    /// `normalAoMain` : normale + AO.
    pub normal_ao: Vec<u32>,
}

/// Compile `<app>/dev_res/texgen/<recipe>.slang`. En cas d'échec, l'erreur contient la
/// sortie de `slangc` (fichier, ligne, message) : c'est ce que lira l'agent.
pub(super) fn compile(recipe: &str, logger: &Logger) -> Result<RecipeShaders> {
    let dirs = AppDirs::get();
    let source = dirs.dev_res().join("texgen").join(format!("{recipe}.slang"));
    ensure!(
        source.is_file(),
        "recette `{recipe}` introuvable : {}",
        source.display()
    );
    let out_dir = dirs.work().join("texgen").join(recipe);
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("création de {}", out_dir.display()))?;
    let surface_spv = out_dir.join("surface.spv");
    let normal_ao_spv = out_dir.join("normal_ao.spv");

    let surface = spawn_slangc(&source, "surfaceMain", &surface_spv)?;
    let normal_ao = spawn_slangc(&source, "normalAoMain", &normal_ao_spv)?;
    // Attendre les deux avant de conclure : un `?` sur le premier laisserait le second
    // tourner sans personne pour le récolter. Les deux compilent le même fichier, donc
    // mêmes erreurs et avertissements : on garde la première erreur, et les
    // avertissements d'une seule passe.
    let surface_result = finish(surface, recipe, Some(logger));
    let normal_ao_result = finish(normal_ao, recipe, None);
    surface_result.and(normal_ao_result)?;

    Ok(RecipeShaders {
        surface: read_spv(&surface_spv)?,
        normal_ao: read_spv(&normal_ao_spv)?,
    })
}

/// Lance `slangc` sur le point d'entrée `entry` de `source`, sans attendre.
fn spawn_slangc(source: &Path, entry: &str, output: &Path) -> Result<Child> {
    Command::new("slangc")
        .arg(source)
        .args(["-I", ENGINE_SHADERS_DIR])
        .args(["-entry", entry, "-stage", "compute", "-o"])
        .arg(output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("lancement de slangc (installé ? dans le PATH ?)")
}

/// Attend `child`. Échec → erreur avec sa sortie ; succès bavard → avertissements
/// loggés si `logger` est fourni.
fn finish(child: Child, recipe: &str, logger: Option<&Logger>) -> Result<()> {
    let output = child.wait_with_output().context("attente de slangc")?;
    // Diagnostics sur stderr d'ordinaire ; stdout gardé au cas où.
    let messages = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let messages = messages.trim();
    if !output.status.success() {
        bail!("compilation de la recette `{recipe}` :\n{messages}");
    }
    if let Some(logger) = logger
        && !messages.is_empty()
    {
        logger.warn(&format!("recette `{recipe}` :\n{messages}"));
    }
    Ok(())
}

/// Lit un `.spv` : `read_spv` vérifie l'en-tête et rend des mots de 32 bits.
fn read_spv(path: &Path) -> Result<Vec<u32>> {
    let bytes = std::fs::read(path).with_context(|| format!("lecture de {}", path.display()))?;
    ash::util::read_spv(&mut std::io::Cursor::new(bytes))
        .with_context(|| format!("SPIR-V invalide : {}", path.display()))
}
