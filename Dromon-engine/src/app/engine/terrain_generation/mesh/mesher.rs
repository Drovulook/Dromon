use crate::app::engine::terrain_generation::chunk::{
    CHUNK_SIZE, ISO_LEVEL, TerrainSnapshot,
};
use crate::app::engine::terrain_generation::lod::Face;
use crate::app::engine::terrain_generation::lod::transition_cells::add_transition_cells;
use crate::app::engine::terrain_generation::marching_cubes::mc_gen::edge_vertex;
use crate::app::engine::terrain_generation::marching_cubes::mc_tables::{CORNERS, TRI_TABLE};
use crate::app::engine::terrain_generation::mesh::vertex::{EdgeKey, MaterialCache, NormalCache};
use crate::app::engine::{
    renderer::render_resources::{MeshData, TerrainVertex},
    terrain_generation::mesh::world_borders::add_mesh_borders,
};
use crate::profile;
use glam::IVec2;
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

/// Altitude du fond du monde (plancher plein). En dessous, on pose la face du
/// dessous ; les parois de bordure montent de là jusqu'au relief.
pub const WORLD_FLOOR: i32 = 0;
/// Rayon, **en pas de pochoir**, des différences pour les normales, moyennées sur
/// `k = 1..=NORMAL_RADIUS`. Plus grand = normale lissée sur une zone plus large
/// (éclairage doux, moins d'aliasing sur les pentes). Dimensionne aussi l'« apron »
/// que le champ de densité doit couvrir autour du chunk.
pub const NORMAL_RADIUS: i32 = 3;

/// Construit le mesh d'un chunk en coordonnées monde (`model` = identité au draw) :
/// la surface (Marching Cubes) plus le fond et, aux bords du monde, les parois
/// latérales qui ferment le volume (cf. [`add_mesh_borders`]).
///
/// Le résultat ne dépend que de `(coord, lod)` et du terrain, jamais des voisins : ils
/// n'interviennent qu'au rendu, par `ChunkSeam`.
pub fn mesh_chunk(terrain: &TerrainSnapshot, lod: u8, coord: IVec2) -> MeshData {
    profile!();
    let n = CHUNK_SIZE as i32;
    let origin_x = coord.x * n;
    let origin_y = coord.y * n;

    // Pas d'échantillonnage MC = `1 << lod` (1, 2, 4…). Il divise CHUNK_SIZE, donc les
    // chunks se tuilent proprement. C'est aussi le pas du pochoir des normales, dont les
    // colonnes tombent ainsi sur la grille déjà calculée (cf. `vertex_normal`).
    let step = 1i32 << lod;
    // Un chunk au LOD maximal n'a jamais de voisin plus grossier.
    let has_slabs = lod < terrain.max_lod();

    // Champ de densité échantillonnable sur la région du chunk. L'apron couvre le
    // pochoir des normales autour des nœuds de bord — au double pas si le chunk porte
    // des dalles, dont la face basse réso prend le pochoir du voisin grossier.
    let stencil_max = if has_slabs { 2 * step } else { step };
    let field = {
        profile!("champ de densité");
        terrain.density_field(coord, NORMAL_RADIUS * stencil_max, step)
    };

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

    // Normales et matériaux sont coûteux (gradient multi-rayon, règles de placement).
    // On les mémoïse par nœud de grille (normales) et par coin entier (matériaux) : les
    // nombreux sommets voisins d'un chunk retombent sur les mêmes clés. `FxHashMap`
    // (hash non-cryptographique) au lieu du `HashMap` SipHash : bien plus rapide sur
    // ces clés entières.
    //
    // Deux caches de matériaux, car deux règles distinctes selon où tombe le sommet : SUR
    // l'iso-surface (profondeur nulle) ou DANS la matière — parois et fond, dont la coupe
    // doit montrer les strates. Un cache commun les mélangerait : le haut d'une paroi est
    // à `z ≈ relief`, donc à la même clé arrondie qu'un sommet de surface.
    let mut normal_cache = NormalCache::default();
    let mut surface_materials = MaterialCache::default();
    let mut volume_materials = MaterialCache::default();

    // Mutualisation des sommets : arête de grille → index. Chaque arête ne produit
    // qu'un sommet, partagé par tous les cubes/triangles qui la touchent.
    let mut vertex_map: FxHashMap<EdgeKey, u32> = FxHashMap::default();

    let mut corner_d = [0.0f32; 8];

    for lx in (0..n).step_by(step as usize) {
        for ly in (0..n).step_by(step as usize) {
            // Tranche utile de CETTE colonne de cubes, et non du chunk entier : en
            // montagne, la tranche du chunk fait des centaines de voxels quand la surface
            // n'en traverse que quelques-uns par colonne. Un cube de départ `z` est plein
            // si `min − (z + step) ≥ ISO`, vide si `max − z < ISO`. Bornes entières par
            // `floor` : sur-ensemble exact (z entier), calé sur le treillis de `z_start`.
            let (z_lo, z_hi) =
                match field.cube_column_relief(origin_x + lx, origin_y + ly, step) {
                    Some((lo, hi)) => {
                        let first = ((lo - ISO_LEVEL).floor() as i32 - step).div_euclid(step) * step;
                        let last = (hi - ISO_LEVEL).floor() as i32;
                        (first.max(z_start), last.min(z_max))
                    }
                    None => (z_start, z_max),
                };
            for lz in (z_lo..=z_hi).step_by(step as usize) {
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

                // `edge(e)` = index du sommet mutualisé de l'arête `e` du cube courant. La
                // clôture referme les 6 arguments d'état (buffers + caches) communs aux
                // 3 appels.
                let mut edge = |e: usize| {
                    edge_vertex(
                        e,
                        base,
                        step,
                        &corner_d,
                        &field,
                        &mut vertices,
                        &mut vertex_map,
                        &mut normal_cache,
                        &mut surface_materials,
                    )
                };

                // Ordre de la table tel quel : ses triangles ont tous leur face avant vers
                // le vide (vérifié sur les 256 cas). Ne pas réorienter d'après une
                // référence unique par cube (gradient au centre) : quand le cube contient
                // deux nappes opposées (crête plus fine qu'un cube, en diagonale), cette
                // référence s'annule et retourne l'une des deux → trous sur les crêtes.
                let mut t = 0;
                while t + 2 < 16 && tris[t] >= 0 {
                    let i0 = edge(tris[t] as usize);
                    let i1 = edge(tris[t + 1] as usize);
                    let i2 = edge(tris[t + 2] as usize);
                    indices.extend_from_slice(&[i0, i1, i2]);
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
    // Elles ne suivent pas la couture (rôle `SEAM_VERTEX_FIXED`) : une face de bord n'a pas
    // de voisin, donc jamais de dalle. ⚠ Mais la caméra peut désormais approcher du bord :
    // un chunk de bord dont le voisin intérieur est plus grossier rétrécit ses sommets de
    // coin, pas son mur — fente possible à ce coin.
    // Le bord du monde se lit dans le monde, **pas** dans la grille : un voisin non
    // chargé (bord de la fenêtre de streaming) ne doit pas faire pousser de mur.
    let border_faces = Face::ALL.map(|f| !terrain.in_world(coord + f.offset()));
    let debug = terrain.mesh_debug();
    {
        profile!("fond et parois");
        add_mesh_borders(
            &field,
            coord,
            border_faces,
            step,
            debug,
            &mut vertices,
            &mut indices,
            &mut volume_materials,
        );
    }

    // Cellules de transition (Transvoxel) : une dalle par face, que le rendu ne dessine que
    // face à un voisin PLUS GROSSIER (le mesh ne dépend donc pas des voisins). Pas de dalle
    // au bord du monde. Table de mutualisation dédiée — leurs positions sont en
    // demi-unités, clés incompatibles avec celles du MC. Aucun index n'est donc partagé
    // avec la surface ci-dessus, mais la déformation au rendu rend les positions confondues
    // (cf. `lib/chunk_seam.slang`) : la couture ferme quand même.
    let body_end = indices.len() as u32;
    let mut slabs = std::array::from_fn(|_| body_end..body_end);
    if has_slabs {
        profile!("transitions");
        // Les cellules de transition n'ajoutent des sommets qu'à la suite (table de
        // mutualisation à part) : tout ce qui est poussé après ce point leur appartient.
        let first_transition_vertex = vertices.len();
        let mut trans_map: FxHashMap<EdgeKey, u32> = FxHashMap::default();
        for (i, face) in Face::ALL.into_iter().enumerate() {
            let start = indices.len() as u32;
            if !border_faces[i] {
                add_transition_cells(
                    &field,
                    coord,
                    face,
                    step,
                    (z_min, z_max),
                    &mut vertices,
                    &mut indices,
                    &mut trans_map,
                    &mut normal_cache,
                    &mut surface_materials,
                );
            }
            slabs[i] = start..indices.len() as u32;
        }
        if debug.transition_color {
            let magenta = field.debug_material();
            for v in &mut vertices[first_transition_vertex..] {
                v.set_material(magenta);
            }
        }
    }

    MeshData {
        vertices,
        indices,
        slabs,
    }
}
