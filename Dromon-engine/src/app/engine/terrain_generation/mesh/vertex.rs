use glam::{IVec3, Vec3};
use rustc_hash::FxHashMap;

use crate::app::engine::terrain_generation::{
    chunk::Voxel, generation::DensityField, mesh::mesher::NORMAL_RADIUS,
};
use crate::profile;

/// Clé d'une arête de la grille : ses deux coins entiers, **triés** (pour que deux
/// cubes voisins partageant l'arête tombent sur la même clé, quel que soit l'ordre de
/// leurs coins). Sert à mutualiser les sommets Marching Cubes : un seul sommet par
/// arête, réutilisé par tous ses triangles (surface indexée ⇒ ~×5 de sommets en moins).
pub type EdgeKey = (i32, i32, i32, i32, i32, i32);

/// Normales mémoïsées par nœud de grille **et** pas de pochoir : un même nœud peut être
/// lu aux deux échelles dans un chunk qui porte une dalle (cf. [`vertex_normal`]).
pub type NormalCache = FxHashMap<(i32, i32, i32, i32), Vec3>;

/// Normale du sommet posé à la fraction `t` de l'arête `a → b` : interpolation des
/// normales de ses deux extrémités, qui sont des nœuds de la grille d'échantillonnage.
///
/// ## Pourquoi aux nœuds, et au pas `stencil`
/// Le pochoir du gradient est espacé de `stencil` (le pas du LOD) : ses colonnes tombent
/// sur la grille que la géométrie a déjà fait calculer. Au pas 1, un sommet LOD 3 payait
/// 12 colonnes de relief neuves (pochoirs isolés, rien de partagé) — 70 % du chunk.
/// Calculer au point exact du sommet plutôt qu'aux nœuds sortirait de la grille dès que
/// le sommet est sur une arête horizontale (pentes).
///
/// Effet visuel : de loin, l'éclairage décrit le relief à la résolution du maillage.
/// Continuité aux frontières de LOD assurée par la dalle Transvoxel, dont la face basse
/// réso prend le pochoir du voisin grossier (cf. `transition_vertex`).
pub fn vertex_normal(
    field: &DensityField,
    cache: &mut NormalCache,
    a: IVec3,
    b: IVec3,
    t: f32,
    stencil: i32,
) -> Vec3 {
    let na = node_normal(field, cache, a, stencil);
    let nb = node_normal(field, cache, b, stencil);
    na.lerp(nb, t).try_normalize().unwrap_or(na)
}

/// `−∇density` normalisé au nœud `c` (pointe vers le vide, où la densité décroît).
/// Différences centrées **moyennées** sur `k = 1..=NORMAL_RADIUS` pas de `stencil`
/// (mélange de pas pairs et impairs → pas de damier d'aliasing).
fn node_normal(field: &DensityField, cache: &mut NormalCache, c: IVec3, stencil: i32) -> Vec3 {
    let key = (c.x, c.y, c.z, stencil);
    if let Some(&n) = cache.get(&key) {
        return n;
    }
    profile!("normale");
    let (wx, wy, wz) = (c.x, c.y, c.z);
    let r = NORMAL_RADIUS.max(1);
    let (mut gx, mut gy, mut gz) = (0.0f32, 0.0f32, 0.0f32);
    for k in 1..=r {
        let d = k * stencil;
        let s = (2 * d) as f32;
        gx += (field.sample(wx + d, wy, wz) - field.sample(wx - d, wy, wz)) / s;
        gy += (field.sample(wx, wy + d, wz) - field.sample(wx, wy - d, wz)) / s;
        gz += (field.sample(wx, wy, wz + d) - field.sample(wx, wy, wz - d)) / s;
    }
    // Le facteur de moyenne 1/r disparaît à la normalisation ; on l'omet.
    let mut n = Vec3::new(-gx, -gy, -gz).normalize_or_zero();
    if n == Vec3::ZERO {
        n = Vec3::Z; // sécurité si le gradient est nul (surface plate dégénérée).
    }
    cache.insert(key, n);
    n
}

/// Matériaux mémoïsés par coin entier **et** échelle de la pente macro (cf.
/// [`DensityField::edge_macro_gradient`]) : dans une dalle, deux sommets de même coin
/// arrondi peuvent être évalués à deux échelles (face haute / basse réso).
pub type MaterialCache = FxHashMap<(i32, i32, i32, i32), Voxel>;

/// Matériaux du sommet `p`, mémoïsés au coin entier le plus proche : les sommets
/// voisins y partagent un mélange quasi identique, économie sans artefact visible. Le
/// calcul lui-même prend la position **flottante** exacte. `scale` = pas de la grille
/// sur laquelle `eval` mesure la pente (0 si elle n'en dépend pas).
///
/// `eval` porte la règle de placement, donc le choix du cache qui va avec :
/// [`DensityField::surface_material`] pour l'iso-surface,
/// [`DensityField::volume_material`] pour les parois et le fond (cf.
/// `surface_materials` / `volume_materials` dans [`mesh_chunk`]).
pub fn cached_material(
    cache: &mut MaterialCache,
    p: Vec3,
    scale: i32,
    eval: impl FnOnce(Vec3) -> Voxel,
) -> Voxel {
    let key = (p.x.round() as i32, p.y.round() as i32, p.z.round() as i32, scale);
    if let Some(&c) = cache.get(&key) {
        return c;
    }
    let c = eval(p);
    cache.insert(key, c);
    c
}
