//! Recette → SPIR-V des deux passes, par `slangc` : un processus par point d'entrée,
//! lancés en parallèle. Le SPIR-V est lu sur la sortie standard, sans fichier (pour
//! l'inspecter : relancer la même commande avec `-o x.spv`).

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
    let recipes_dir = AppDirs::get().dev_res().join("texgen");
    let source = recipes_dir.join(format!("{recipe}.slang"));
    ensure!(
        source.is_file(),
        "recette `{recipe}` introuvable : {}",
        source.display()
    );

    let surface = spawn_slangc(&source, &recipes_dir, "surfaceMain")?;
    let normal_ao = spawn_slangc(&source, &recipes_dir, "normalAoMain")?;
    // Attendre les deux avant le premier `?` : il laisserait sinon le second tourner sans
    // personne pour le récolter. Les deux compilent le même fichier, donc mêmes erreurs
    // et avertissements : on garde la première erreur, et les avertissements d'une seule
    // passe.
    let surface = finish(surface, recipe, Some(logger));
    let normal_ao = finish(normal_ao, recipe, None);
    let (surface, normal_ao) = (surface?, normal_ao?);

    Ok(RecipeShaders {
        surface: read_spv(surface, "surfaceMain")?,
        normal_ao: read_spv(normal_ao, "normalAoMain")?,
    })
}

/// Lance `slangc` sur le point d'entrée `entry` de `source`, sans attendre. Sans `-o`,
/// le SPIR-V part sur stdout ; les diagnostics, sur stderr.
/// Includes cherchés depuis `recipes_dir` (fichiers partagés de l'app, `lib/…`) puis
/// depuis les shaders du moteur (`procgen/…`) : même chemin quel que soit le fichier
/// qui inclut (un include relatif seul partirait du dossier de ce fichier).
fn spawn_slangc(source: &Path, recipes_dir: &Path, entry: &str) -> Result<Child> {
    Command::new("slangc")
        .arg(source)
        .arg("-I")
        .arg(recipes_dir)
        .args(["-I", ENGINE_SHADERS_DIR])
        .args(["-entry", entry, "-stage", "compute", "-target", "spirv"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("lancement de slangc (installé ? dans le PATH ?)")
}

/// Attend `child` et rend sa sortie standard (le SPIR-V). Échec → erreur avec ses
/// diagnostics ; succès bavard → avertissements loggés si `logger` est fourni.
fn finish(child: Child, recipe: &str, logger: Option<&Logger>) -> Result<Vec<u8>> {
    let output = child.wait_with_output().context("attente de slangc")?;
    let messages = String::from_utf8_lossy(&output.stderr);
    let messages = messages.trim();
    if !output.status.success() {
        bail!("compilation de la recette `{recipe}` :\n{messages}");
    }
    if let Some(logger) = logger
        && !messages.is_empty()
    {
        logger.warn(&format!("recette `{recipe}` :\n{messages}"));
    }
    Ok(output.stdout)
}

/// `read_spv` vérifie l'en-tête SPIR-V et rend des mots de 32 bits.
fn read_spv(bytes: Vec<u8>, entry: &str) -> Result<Vec<u32>> {
    ash::util::read_spv(&mut std::io::Cursor::new(bytes))
        .with_context(|| format!("SPIR-V invalide en sortie de slangc ({entry})"))
}
