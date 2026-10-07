//! Suppression des **petites régions** (mers intérieures, îlots) : une question de
//! connexité sur toute la carte, qu'un bruit point par point ignore.

/// Bascule (`!value`) chaque région connexe de cellules valant `value` qui ne touche
/// pas le bord de la grille, compte moins de `min_cells` cellules et pour laquelle
/// `remove(première cellule)` est vrai. Rend le nombre de régions basculées.
///
/// `diagonals` : connexité 8 au lieu de 4. Toujours l'inverse pour la terre et la
/// mer (terre 8, mer 4), sinon deux régions se croisent par un coin
/// (`T M / M T`) : connexes toutes deux, ou aucune.
pub fn flip_small_regions(
    mask: &mut [bool],
    n: usize,
    value: bool,
    diagonals: bool,
    min_cells: usize,
    remove: impl Fn(usize) -> bool,
) -> usize {
    // Les 4 premiers : orthogonaux ; les 4 suivants : diagonales.
    const OFFSETS: [(isize, isize); 8] = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    let offsets = if diagonals { &OFFSETS[..] } else { &OFFSETS[..4] };

    let mut visited = vec![false; n * n];
    let mut stack = Vec::new();
    let mut region = Vec::new();
    let mut flipped = 0;

    for start in 0..n * n {
        if mask[start] != value || visited[start] {
            continue;
        }
        // Parcours en profondeur (pile explicite : pas de récursion sur 10⁶ cellules).
        visited[start] = true;
        stack.push(start);
        region.clear();
        let mut touches_edge = false;
        while let Some(k) = stack.pop() {
            region.push(k);
            let (i, j) = ((k % n) as isize, (k / n) as isize);
            let last = n as isize - 1;
            touches_edge |= i == 0 || j == 0 || i == last || j == last;
            for &(di, dj) in offsets {
                let (ni, nj) = (i + di, j + dj);
                if ni < 0 || nj < 0 || ni > last || nj > last {
                    continue;
                }
                let nb = nj as usize * n + ni as usize;
                if mask[nb] == value && !visited[nb] {
                    visited[nb] = true;
                    stack.push(nb);
                }
            }
        }
        // Mer touchant le bord = l'océan (garanti au bord, cf. `ContinentField`).
        if !touches_edge && region.len() < min_cells && remove(start) {
            for &k in &region {
                mask[k] = !value;
            }
            flipped += 1;
        }
    }
    flipped
}
