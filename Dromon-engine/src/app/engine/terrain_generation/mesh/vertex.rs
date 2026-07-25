use glam::Vec3;
use rustc_hash::FxHashMap;

use crate::app::engine::terrain_generation::{
    generation::DensityField, mesh::mesher::NORMAL_RADIUS,
};

/// Clé d'une arête de la grille : ses deux coins entiers, **triés** (pour que deux
/// cubes voisins partageant l'arête tombent sur la même clé, quel que soit l'ordre de
/// leurs coins). Sert à mutualiser les sommets Marching Cubes : un seul sommet par
/// arête, réutilisé par tous ses triangles (surface indexée ⇒ ~×5 de sommets en moins).
pub type EdgeKey = (i32, i32, i32, i32, i32, i32);

/// Normale de surface au sommet `p` : `−∇density` normalisé (pointe vers le vide, où
/// la densité décroît). Gradient estimé par différences centrées **moyennées** sur
/// `k = 1..=NORMAL_RADIUS` (mélange de pas pairs et impairs → pas de damier
/// d'aliasing). Échantillonné au coin entier le plus proche et mémoïsé.
pub fn vertex_normal(
    field: &DensityField,
    cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
    p: Vec3,
) -> Vec3 {
    let key = (p.x.round() as i32, p.y.round() as i32, p.z.round() as i32);
    if let Some(&n) = cache.get(&key) {
        return n;
    }
    let (wx, wy, wz) = key;
    let r = NORMAL_RADIUS.max(1);
    let (mut gx, mut gy, mut gz) = (0.0f32, 0.0f32, 0.0f32);
    for k in 1..=r {
        let s = (2 * k) as f32;
        gx += (field.sample(wx + k, wy, wz) - field.sample(wx - k, wy, wz)) / s;
        gy += (field.sample(wx, wy + k, wz) - field.sample(wx, wy - k, wz)) / s;
        gz += (field.sample(wx, wy, wz + k) - field.sample(wx, wy, wz - k)) / s;
    }
    // Le facteur de moyenne 1/r disparaît à la normalisation ; on l'omet.
    let mut n = Vec3::new(-gx, -gy, -gz).normalize_or_zero();
    if n == Vec3::ZERO {
        n = Vec3::Z; // sécurité si le gradient est nul (surface plate dégénérée).
    }
    cache.insert(key, n);
    n
}

/// Couleur du sommet `p`, mémoïsée au coin entier le plus proche : les sommets voisins
/// y partagent une couleur quasi identique, économie sans artefact visible. Le calcul
/// lui-même prend la position **flottante** exacte.
///
/// `eval` porte la règle de coloration, donc le choix du cache qui va avec :
/// [`DensityField::surface_color`] pour l'iso-surface, [`DensityField::volume_color`] pour les
/// parois et le fond (cf. `surface_colors` / `volume_colors` dans [`mesh_chunk`]).
pub fn cached_color(
    cache: &mut FxHashMap<(i32, i32, i32), Vec3>,
    p: Vec3,
    eval: impl FnOnce(Vec3) -> Vec3,
) -> Vec3 {
    let key = (p.x.round() as i32, p.y.round() as i32, p.z.round() as i32);
    if let Some(&c) = cache.get(&key) {
        return c;
    }
    let c = eval(p);
    cache.insert(key, c);
    c
}
