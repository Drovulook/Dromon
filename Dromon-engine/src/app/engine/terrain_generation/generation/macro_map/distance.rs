//! **Distance transform** : distance de chaque cellule à la plus proche cellule
//! « graine », mesurée dans l'espace et non déduite d'un bruit.
//!
//! Propagation de proche en proche (Danielsson, 8SSEDT) : chaque cellule retient
//! les coordonnées de sa graine la plus proche connue et adopte celle d'un voisin si
//! elle est plus proche. Deux balayages suffisent : haut → bas puis bas → haut,
//! chacun complété d'un aller-retour horizontal sur la ligne. Quasi exact (erreur
//! rare, < 1 cellule), O(n²).

/// Coordonnée d'une graine absente : assez loin pour perdre toute comparaison,
/// assez proche pour que `dx² + dy²` tienne dans un `i64`.
const FAR: i32 = -1_000_000;

/// Distance euclidienne, en cellules, de chaque cellule à la plus proche cellule
/// où `seeds` est vrai (0 sur les graines). Grille `n × n` rangée `[j·n + i]`.
pub fn distance_to(seeds: &[bool], n: usize) -> Vec<f32> {
    let n = n as i32;
    let mut nearest: Vec<(i32, i32)> = seeds
        .iter()
        .enumerate()
        .map(|(k, &s)| {
            if s {
                (k as i32 % n, k as i32 / n)
            } else {
                (FAR, FAR)
            }
        })
        .collect();

    // Haut → bas (j croissant) : voisins déjà traités au-dessus et à gauche, puis retour à droite.
    for j in 0..n {
        for i in 0..n {
            for (di, dj) in [(-1, 0), (-1, -1), (0, -1), (1, -1)] {
                relax(&mut nearest, n, i, j, di, dj);
            }
        }
        for i in (0..n).rev() {
            relax(&mut nearest, n, i, j, 1, 0);
        }
    }
    // Bas → haut : symétrique.
    for j in (0..n).rev() {
        for i in (0..n).rev() {
            for (di, dj) in [(1, 0), (1, 1), (0, 1), (-1, 1)] {
                relax(&mut nearest, n, i, j, di, dj);
            }
        }
        for i in 0..n {
            relax(&mut nearest, n, i, j, -1, 0);
        }
    }

    nearest
        .iter()
        .enumerate()
        .map(|(k, &s)| (dist2(s, k as i32 % n, k as i32 / n) as f32).sqrt())
        .collect()
}

/// La cellule `(i, j)` adopte la graine de son voisin `(i + di, j + dj)` si elle
/// est plus proche que la sienne.
fn relax(nearest: &mut [(i32, i32)], n: i32, i: i32, j: i32, di: i32, dj: i32) {
    let (ni, nj) = (i + di, j + dj);
    if ni < 0 || nj < 0 || ni >= n || nj >= n {
        return;
    }
    let candidate = nearest[(nj * n + ni) as usize];
    let k = (j * n + i) as usize;
    if dist2(candidate, i, j) < dist2(nearest[k], i, j) {
        nearest[k] = candidate;
    }
}

fn dist2((si, sj): (i32, i32), i: i32, j: i32) -> i64 {
    let (dx, dy) = ((si - i) as i64, (sj - j) as i64);
    dx * dx + dy * dy
}

/// Distance **signée** à la côte, en cellules : > 0 sur terre, < 0 en mer. La côte
/// passe à mi-chemin entre une cellule de terre et une cellule de mer, d'où le ½.
pub fn signed_coast_distance(land: &[bool], n: usize) -> Vec<f32> {
    let sea: Vec<bool> = land.iter().map(|&l| !l).collect();
    let to_sea = distance_to(&sea, n);
    let to_land = distance_to(land, n);
    land.iter()
        .zip(to_sea.iter().zip(&to_land))
        .map(|(&l, (&ds, &dl))| if l { ds - 0.5 } else { 0.5 - dl })
        .collect()
}
