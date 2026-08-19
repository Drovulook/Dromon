use crate::app::engine::terrain_generation::chunk::{CHUNK_SIZE, ISO_LEVEL, TerrainSnapshot};
use crate::app::engine::terrain_generation::lod::Face;
use crate::app::engine::terrain_generation::lod::grid::LodGrid;
use crate::app::engine::terrain_generation::lod::transition_cells::add_transition_cells;
use crate::app::engine::terrain_generation::lod::transition_shrink::HalfStepShrink;
use crate::app::engine::terrain_generation::marching_cubes::mc_gen::edge_vertex;
use crate::app::engine::terrain_generation::marching_cubes::mc_tables::{
    CORNERS, EDGE_CORNERS, TRI_TABLE,
};
use crate::app::engine::terrain_generation::mesh::vertex::EdgeKey;
use crate::app::engine::{
    renderer::render_resources::{MeshData, TerrainVertex},
    terrain_generation::mesh::world_borders::add_mesh_borders,
};
use crate::profile;
use glam::{IVec2, IVec3, Vec3};
use rustc_hash::FxHashMap;

// ─── Mailleur volumétrique (Marching Cubes) ──────────────────────────────────
//
// On **extrait l'iso-surface** d'un champ de densité 3D ([`DensityField`]). Le
// mailleur ne connaît QUE ce champ : `sample(x,y,z)` (la densité) et
// `vertical_bounds()` (la tranche en z à parcourir). Il ignore tout du relief, des
// grottes, des édits — c'est le champ qui les porte. Ajouter des grottes ne touchera
// donc pas ce fichier : dès que `DensityField::sample` renverra du vide en
// profondeur, Marching Cubes y posera des triangles automatiquement (grottes,
// surplombs, cavités détachées), dans le même buffer de chunk.
//
// Principe : pour chaque cube de la grille, on regarde le signe de la densité à ses
// 8 coins ; là où la surface traverse une arête, on pose un sommet par interpolation
// linéaire. Couture sans fissure : la densité ne dépend que de la position MONDE,
// donc deux chunks voisins calculent des coins identiques sur leur face partagée.

/// Rayon (en voxels) du stencil de différences pour les normales, moyenné sur
/// Altitude du fond du monde (plancher plein). En dessous, on pose la face du
/// dessous ; les parois de bordure montent de là jusqu'au relief.
pub const WORLD_FLOOR: i32 = 0;
/// `k = 1..=NORMAL_RADIUS`. Plus grand = normale lissée sur une zone plus large
/// (éclairage doux, moins d'aliasing sur les pentes). Dimensionne aussi l'« apron »
/// que le champ de densité doit pré-échantillonner autour du chunk.
pub const NORMAL_RADIUS: i32 = 5;

/// Construit le mesh d'un chunk en coordonnées monde (`model` = identité au draw) :
/// la surface (Marching Cubes) plus le fond et, aux bords du monde, les parois
/// latérales qui ferment le volume (cf. [`add_mesh_borders`]).
///
/// Les deux entrées sont **en lecture seule et sans état** : `terrain` porte le relief
/// (immuable), `lods` la configuration de niveaux du lot en cours. C'est ce qui permet
/// d'appeler cette fonction depuis un thread de fond pendant que la caméra bouge — le
/// lot maille contre la grille qu'on lui a passée, pas contre la plus récente.
pub fn mesh_chunk(terrain: &TerrainSnapshot, lods: &LodGrid, coord: IVec2) -> MeshData {
    profile!();
    let n = CHUNK_SIZE as i32;
    let origin_x = coord.x * n;
    let origin_y = coord.y * n;

    // Pas d'échantillonnage MC = `1 << lod` (1, 2, 4…). Il divise CHUNK_SIZE, donc les
    // chunks se tuilent proprement. N'affecte QUE la géométrie : le stencil des normales
    // reste à un pas fixe de 1 unité (éclairage continu à travers une frontière de LOD).
    let step = 1i32 << lods.lod(coord);

    // Champ de densité échantillonnable sur la région du chunk. L'apron vaut le rayon
    // des normales : le stencil de différences ne débordera jamais du pré-échantillon.
    let field = terrain.density_field(coord, NORMAL_RADIUS);

    // Tranche verticale à mailler, fournie par le champ : hors d'elle, tout est plein
    // (dessous) ou vide (dessus). Se resserre automatiquement autour de la surface —
    // et s'élargira vers le bas quand les grottes creuseront en profondeur.
    let (z_min, z_max) = field.vertical_bounds();

    // Calage du départ z sur un treillis global multiple de `step`. En X/Y les colonnes
    // tombent déjà sur un treillis global (origin = coord·64, 64 divisible par step) ;
    // pas en Z. Sans ce cadrage, deux chunks voisins dont les `z_min` ont des parités
    // différentes échantillonneraient des z décalés sur leur face commune → fissure même
    // à LOD égal. `div_euclid` arrondit vers le bas au multiple de step (sûr si négatif).
    let z_start = z_min.div_euclid(step) * step;

    let mut vertices: Vec<TerrainVertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // Normales et couleurs sont coûteuses (gradient multi-rayon, choix de matériau).
    // On les mémoïse par coin entier : les nombreux sommets voisins d'un chunk
    // retombent sur les mêmes clés → calcul fait ~une fois par colonne. `FxHashMap`
    // (hash non-cryptographique) au lieu du `HashMap` SipHash : bien plus rapide sur
    // ces clés entières.
    //
    // Deux caches de couleur, car deux règles distinctes selon où tombe le sommet : SUR
    // l'iso-surface (profondeur nulle) ou DANS la matière — parois et fond, dont la coupe
    // doit montrer les strates. Un cache commun les mélangerait : le haut d'une paroi est
    // à `z ≈ relief`, donc à la même clé arrondie qu'un sommet de surface.
    let mut normal_cache: FxHashMap<(i32, i32, i32), Vec3> = FxHashMap::default();
    let mut surface_colors: FxHashMap<(i32, i32, i32), Vec3> = FxHashMap::default();
    let mut volume_colors: FxHashMap<(i32, i32, i32), Vec3> = FxHashMap::default();

    // Mutualisation des sommets : arête de grille → index. Chaque arête ne produit
    // qu'un sommet, partagé par tous les cubes/triangles qui la touchent.
    let mut vertex_map: FxHashMap<EdgeKey, u32> = FxHashMap::default();

    // Faces à coudre — connues AVANT le maillage : elles conditionnent le
    // rétrécissement demi-pas que subit la dernière rangée de sommets qui les borde.
    let lod_transition_faces = lods.faces(coord);
    let shrink = HalfStepShrink::new(lods, coord, lod_transition_faces, step);

    let mut corner_d = [0.0f32; 8];

    for lx in (0..n).step_by(step as usize) {
        for ly in (0..n).step_by(step as usize) {
            for lz in (z_start..=z_max).step_by(step as usize) {
                let base = [origin_x + lx, origin_y + ly, lz];

                // Densité aux 8 coins → index de cas (bit posé quand le coin est
                // sous l'iso = vide, convention de la table de Bourke). Les coins sont
                // espacés de `step` (cube MC de côté `step` unités monde).
                let mut cube_index = 0usize;
                for (i, off) in CORNERS.iter().enumerate() {
                    let d = field.sample(
                        base[0] + off[0] * step,
                        base[1] + off[1] * step,
                        base[2] + off[2] * step,
                    );
                    corner_d[i] = d;
                    if d < ISO_LEVEL {
                        cube_index |= 1 << i;
                    }
                }

                let tris = &TRI_TABLE[cube_index];
                if tris[0] < 0 {
                    continue; // cube entièrement plein ou entièrement vide.
                }

                // `edge(e)` = sommet mutualisé de l'arête `e` du cube courant. La
                // clôture referme les 6 arguments d'état (buffers + caches) communs aux
                // 3 appels. `edge_vertex` renvoie des valeurs possédées → aucun emprunt
                // de `vertices` ne fuit hors de l'appel (sinon le test d'orientation, qui
                // lit `vertices`, entrerait en conflit avec l'emprunt `&mut`).
                let mut edge = |e: usize| {
                    edge_vertex(
                        e,
                        base,
                        step,
                        shrink,
                        &corner_d,
                        &field,
                        &mut vertices,
                        &mut vertex_map,
                        &mut normal_cache,
                        &mut surface_colors,
                    )
                };

                // Référence d'enroulement : `−∇d` du champ trilinéaire du cube, lu sur ses
                // 8 densités (à un facteur > 0 près, car `Σ (coin − centre) = 0`). Donc
                // À L'ÉCHELLE DU CUBE — surtout pas les normales de sommet, dont le stencil
                // reste à 5 unités quel que soit le LOD : à pas grossier elles décrivent un
                // relief plus fin que la géométrie et peuvent s'écarter de plus de 90° du
                // triangle, ce qui retournait des triangles (trous noirs à LOD élevé).
                let mut winding_ref = Vec3::ZERO;
                for (i, off) in CORNERS.iter().enumerate() {
                    let c = Vec3::new(off[0] as f32, off[1] as f32, off[2] as f32) - 0.5;
                    winding_ref -= c * corner_d[i];
                }

                let mut t = 0;
                while t + 2 < 16 && tris[t] >= 0 {
                    let (i0, p0, _) = edge(tris[t] as usize);
                    let (i1, p1, _) = edge(tris[t + 1] as usize);
                    let (i2, p2, _) = edge(tris[t + 2] as usize);

                    // Oriente l'enroulement vers le vide : on permute les INDEX.
                    let geo = (p1 - p0).cross(p2 - p0);
                    if geo.dot(winding_ref) < 0.0 {
                        indices.extend_from_slice(&[i0, i2, i1]);
                    } else {
                        indices.extend_from_slice(&[i0, i1, i2]);
                    }

                    t += 3;
                }
            }
        }
    }

    // Fermeture du volume : fond (partout) + parois sur les côtés qui touchent le bord
    // du monde. Ces faces sont planes et explicites (pas de Marching Cubes) → murs nets,
    // et elles se cousent à la surface car leur arête haute est à `z = relief` aux mêmes
    // points entiers que les sommets de bord du maillage MC.
    //
    // Elles ignorent `shrink` : le bord du monde est le point le plus éloigné du focus,
    // donc uniformément au LOD le plus grossier ⇒ aucun voisin plus grossier, aucune
    // dalle sur ces faces, rien à rétrécir. À revoir si le focus peut s'en approcher.
    // Une face est au bord du monde si le chunk voisin de ce côté n'est pas chargé.
    let border_faces = Face::ALL.map(|f| !lods.is_loaded(coord + f.offset()));
    add_mesh_borders(
        &field,
        coord,
        border_faces,
        step,
        &mut vertices,
        &mut indices,
        &mut volume_colors,
    );

    // Cellules de transition (Transvoxel) : scellent les faces bordant un voisin PLUS
    // GROSSIER. Table de mutualisation dédiée — leurs positions sont en demi-unités,
    // clés incompatibles avec celles du MC. Aucun index n'est donc partagé avec la
    // surface ci-dessus, mais le rétrécissement demi-pas a rendu les positions
    // confondues (cf. [`HalfStepShrink`]) : la couture ferme quand même.
    if !lod_transition_faces.is_empty() {
        let mut trans_map: FxHashMap<EdgeKey, u32> = FxHashMap::default();
        for face in lod_transition_faces.iter() {
            debug_assert_eq!(
                lods.lod(coord + face.offset()),
                lods.lod(coord) + 1,
                "Transvoxel ne coud qu'un niveau d'écart (cf. LodGrid::rebalance)"
            );
            add_transition_cells(
                &field,
                coord,
                face,
                step,
                shrink,
                (z_min, z_max),
                &mut vertices,
                &mut indices,
                &mut trans_map,
                &mut normal_cache,
                &mut surface_colors,
            );
        }
    }

    MeshData { vertices, indices }
}

#[cfg(test)]
mod tests {
    use crate::GenParams;
    use crate::app::engine::terrain_generation::chunk::{CHUNK_SIZE, ChunkStore, TerrainSource};
    use crate::app::engine::terrain_generation::lod::Face;
    use crate::app::engine::terrain_generation::lod::transition_shrink::HalfStepShrink;
    use std::sync::Arc;

    use super::*;

    /// Grille de LOD d'un petit monde de test : niveaux imposés explicitement, puis
    /// équilibrage et détection des coutures comme en production.
    fn lod_grid(chunks: &[(IVec2, u8)]) -> LodGrid {
        let mut grid = LodGrid::new(chunks.iter().map(|&(c, _)| c).collect());
        grid.set_raw_lods(|c, _| chunks.iter().find(|&&(k, _)| k == c).unwrap().1);
        grid.rebalance();
        grid
    }

    /// Maille deux chunks voisins de LOD 0 et 1 (le fin à l'ouest) et rend leurs sommets.
    /// Le fin porte donc une face de transition vers l'est, le grossier aucune.
    ///
    /// La paire est isolée, donc au bord du monde de tous les côtés sauf le plan
    /// frontière qu'inspectent les tests — celui-ci reste libre de toute paroi (et les
    /// parois sont de toute façon désactivées, cf. `GENERATE_WORLD_WALLS`). Y demeurent
    /// la surface et le fond à `WORLD_FLOOR`.
    fn stitched_pair() -> (Vec<TerrainVertex>, Vec<TerrainVertex>) {
        let (fine, coarse) = (IVec2::new(0, 0), IVec2::new(1, 0));
        let source = Arc::new(TerrainSource::new(GenParams::default()));
        let terrain = TerrainSnapshot::new(&source, &ChunkStore::default());
        let lods = lod_grid(&[(fine, 0), (coarse, 1)]);

        (
            mesh_chunk(&terrain, &lods, fine).vertices,
            mesh_chunk(&terrain, &lods, coarse).vertices,
        )
    }

    /// La face de transition est bien détectée sur le plan partagé (garde-fou : sans
    /// elle, le test d'étanchéité ci-dessous passerait pour de mauvaises raisons).
    #[test]
    fn fine_chunk_stitches_toward_the_coarse_neighbor() {
        let lods = lod_grid(&[(IVec2::new(0, 0), 0), (IVec2::new(1, 0), 1)]);

        let faces = lods.faces(IVec2::new(0, 0));
        assert!(faces.contains(Face::PosX));
        assert_eq!(faces.iter().count(), 1);
    }

    /// **Étanchéité de la couture.** Tout sommet que le chunk GROSSIER pose sur le plan
    /// frontière doit se retrouver chez le fin : la face basse réso de la dalle reprend
    /// les 4 coins du voisin, donc ses arêtes ET ses interpolations — le bord est le même
    /// *par construction*, ce qui est toute la raison d'être du Transvoxel.
    ///
    /// Comparaison à tolérance (et non bit-à-bit) : les deux côtés interpolent la même
    /// arête, mais rien ne garantit qu'ils la parcourent dans le même sens, et
    /// `a + (b - a)·t` n'est pas bit-identique à `b + (a - b)·(1 - t)` en flottant.
    #[test]
    fn transition_reproduces_the_coarse_border() {
        let (fine, coarse) = stitched_pair();
        let border = CHUNK_SIZE as f32;

        let on_border: Vec<Vec3> = fine
            .iter()
            .map(|v| v.pos)
            .filter(|p| (p.x - border).abs() < 1e-3)
            .collect();

        let mut checked = 0;
        for v in coarse.iter().filter(|v| (v.pos.x - border).abs() < 1e-3) {
            assert!(
                on_border.iter().any(|p| p.distance(v.pos) < 1e-3),
                "sommet {:?} du bord grossier absent de la couture → fente",
                v.pos
            );
            checked += 1;
        }
        assert!(
            checked > 10,
            "seulement {checked} sommets sur la frontière : le test ne prouve rien"
        );
    }

    /// La formule de compression, isolée : le plan frontière recule d'un demi-pas (là
    /// où la dalle pose sa face haute réso), le fond de la rangée ne bouge pas
    /// (continuité avec le reste du chunk), et au-delà d'un pas rien n'est touché.
    #[test]
    fn half_step_shrink_compresses_only_the_border_row() {
        // (0,0) en LOD 2 (donc step 4) avec un voisin est plus grossier ⇒ dalle à l'est.
        // Ses voisins nord/sud restent absents : ils ne portent aucune dalle est, donc
        // l'atténuation s'applique aux deux extrémités de la face.
        let (me, east) = (IVec2::ZERO, IVec2::new(1, 0));
        let lods = lod_grid(&[(me, 2), (east, 3)]);

        let step = 4;
        let border = CHUNK_SIZE as f32; // face est du chunk (0,0)
        let shrink = HalfStepShrink::new(&lods, me, lods.faces(me), step);
        // Sommet à distance `d` du plan frontière → sa coordonnée x après compression.
        // `y = 10` : au-delà de la rampe d'atténuation, amplitude pleine.
        let x = |d: f32| shrink.apply(Vec3::new(border - d, 10.0, 10.0)).x;

        assert_eq!(x(0.0), border - 2.0); // bord extérieur : recule de step/2
        assert_eq!(x(2.0), border - 3.0); // milieu : rangée comprimée ×2, linéairement
        assert_eq!(x(step as f32), border - 4.0); // bord intérieur : inchangé
        assert_eq!(x(9.0), border - 9.0); // hors rangée : intact

        // Atténuation : sur les plans frontières nord et sud (y = 0 et y = 64), partagés
        // avec des voisins qui ne comprimeraient pas, le déplacement doit être NUL.
        for y in [0.0, CHUNK_SIZE as f32] {
            let p = Vec3::new(border, y, 10.0);
            assert_eq!(shrink.apply(p), p, "contour déplacé sur un plan frontière");
        }
        // À mi-rampe, moitié de l'amplitude.
        assert_eq!(shrink.apply(Vec3::new(border, 2.0, 10.0)).x, border - 1.0);

        // Rien sur les axes sans dalle (ici NegX, NegY, PosY) ni jamais sur Z.
        let p = Vec3::new(10.0, 0.0, 0.0);
        assert_eq!(shrink.apply(p), p);
    }

    /// **Soudure intérieure** — le pendant de [`transition_reproduces_the_coarse_border`],
    /// qui ne teste que la face BASSE réso de la dalle (côté voisin). Ici la face HAUTE
    /// réso : le rétrécissement doit avoir vidé le plan frontière de ses sommets de
    /// surface, tous reculés à `frontière − step/2` — précisément là où la dalle pose les
    /// siens, sur les mêmes arêtes et avec les mêmes densités. Chacun y a donc un jumeau.
    #[test]
    fn shrunk_border_welds_onto_the_transition_slab() {
        let (fine, _) = stitched_pair();
        let border = CHUNK_SIZE as f32;
        let step = 1.0; // le chunk fin est en LOD 0.

        // Deux exclusions : le fond du monde, qui touche le plan frontière sans être
        // rétréci (aucune dalle en face de lui) ; et les deux bouts de la face, où
        // l'atténuation réduit le retrait (cf. [`HalfStepShrink`]) — le chunk fin n'a ici
        // aucun voisin nord/sud, donc l'atténuation y est active.
        let surface: Vec<Vec3> = fine
            .iter()
            .map(|v| v.pos)
            .filter(|p| p.z > WORLD_FLOOR as f32 + 1.0)
            .filter(|p| p.y > step && p.y < CHUNK_SIZE as f32 - step)
            .collect();

        // Le plan frontière lui-même n'est PAS vide : la face basse réso de la dalle y
        // reste, c'est elle qui épouse le voisin. Ce qui doit avoir disparu, c'est la
        // bande ouverte `(frontière − step/2, frontière)` : la compression envoie tout
        // sommet MC à une distance ≥ `step/2` du plan, et la dalle n'y pose rien.
        assert!(
            !surface
                .iter()
                .any(|p| p.x > border - step / 2.0 + 1e-4 && p.x < border - 1e-4),
            "un sommet de surface occupe la bande que la dalle traverse → recouvrement"
        );

        let welded: Vec<Vec3> = surface
            .into_iter()
            .filter(|p| (p.x - (border - step / 2.0)).abs() < 1e-4)
            .collect();
        assert!(
            welded.len() > 10,
            "seulement {} sommets sur le plan rétréci : le test ne prouve rien",
            welded.len()
        );

        for p in &welded {
            // ≥ 2 : le sommet MC et celui de la dalle, confondus mais d'index distincts
            // (tables de mutualisation séparées). `p` se compte lui-même.
            let twins = welded.iter().filter(|q| p.distance(**q) < 1e-4).count();
            assert!(
                twins >= 2,
                "sommet {p} seul sur le plan rétréci → couture ouverte côté intérieur"
            );
        }
    }
}
