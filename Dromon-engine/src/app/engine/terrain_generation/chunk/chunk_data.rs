//! **Données non procédurales d'un chunk** — tout ce qui ne se recalcule pas depuis
//! `(seed, coord)`, donc tout ce qu'il faudra sauvegarder.
//!
//! ## Pourquoi par chunk, et pas une table globale
//! La table vivait autrefois dans le `ChunkManager`, en coordonnées monde. Le
//! court-circuit de [`DensityField::sample`](super::super::generation::DensityField::sample)
//! (« aucun édit ⇒ chemin gratuit ») était alors **global** : un seul trou creusé
//! quelque part le désarmait pour le monde entier, et les ~1,3 million d'échantillons
//! par chunk repayaient tous un hachage. Par chunk, le court-circuit redevient local et
//! les 99,9 % de chunks intacts gardent le chemin gratuit.
//!
//! Deux autres raisons, structurelles : c'est l'unité de **persistance** (un chunk
//! déchargé jette son mesh et garde ses édits), et l'unité de **copie à l'écriture** —
//! éditer clone quelques Ko au lieu de la table du monde (cf. [`ChunkStore`]).
//!
//! [`ChunkStore`]: super::chunk_store::ChunkStore

use super::{CHUNK_HEIGHT, CHUNK_SIZE};
use glam::IVec3;
use rustc_hash::FxHashMap;

/// Contenu **source de vérité** d'un chunk : ce que le procédural ne sait pas rendre.
///
/// ⚠ N'y mettre que du non-recalculable. Le mesh, les normales ou les props générés
/// depuis la graine sont *dérivés* : ils vivent côté rendu et se jettent librement.
#[derive(Clone, Default)]
pub struct ChunkData {
    /// Densités modifiées par le joueur (creuser/remblayer), prioritaires sur le
    /// procédural. Vide tant qu'on n'a pas édité — le cas de l'immense majorité.
    ///
    /// Clés en coordonnées **locales** au chunk, pas monde : le chunk reste
    /// relocalisable et sérialisable tel quel, sans dépendre de la position où il a été
    /// écrit. La conversion se fait de toute façon aux frontières (cf. `split_world` et
    /// `region_edits` dans [`ChunkStore`](super::chunk_store::ChunkStore)).
    edits: FxHashMap<IVec3, f32>,
}

impl ChunkData {
    /// Écrit une densité en coordonnées **locales** au chunk.
    pub fn set_edit(&mut self, local: IVec3, density: f32) {
        debug_assert!(
            (0..CHUNK_SIZE as i32).contains(&local.x)
                && (0..CHUNK_SIZE as i32).contains(&local.y)
                && (0..CHUNK_HEIGHT as i32).contains(&local.z),
            "coordonnée voxel hors du chunk (coordonnées monde passées par erreur ?) : {local}"
        );
        self.edits.insert(local, density);
    }

    /// Les édits, en coordonnées **locales**.
    pub fn edits(&self) -> impl Iterator<Item = (IVec3, f32)> + '_ {
        self.edits.iter().map(|(&c, &d)| (c, d))
    }
}
