//! **Politique de LOD** du terrain — fonctions pures sur coordonnées/distances, sans
//! rien connaître du rendu ni du maillage. `world.rs` ne fait qu'orchestrer ; c'est ici
//! que vit la règle « distance → niveau de détail », les anneaux et l'hystérésis.
//!
//! Un niveau de LOD `l` correspond à un pas d'échantillonnage `step = 1 << l` : LOD0 =
//! pleine résolution (÷1), LOD1 = ÷4 sommets, LOD2 = ÷16 (la surface est une nappe 2D,
//! doubler le pas quadruple l'aire couverte par cellule).
//!
//! Le résultat de cette politique se range dans une [`grid::LodGrid`] ; c'est
//! [`lod_updater::LodUpdater`] qui la recalcule quand la caméra a assez bougé.

pub mod grid;
pub mod lod_updater;
pub mod transition_cells;
pub mod transition_shrink;
mod transvoxel_tables;

use super::chunk::CHUNK_SIZE;
use glam::{IVec2, Vec2, Vec3};

/// Niveau de détail maximal (pas = `1 << MAX_LOD`). 3 paliers pour commencer.
pub const MAX_LOD: u8 = 3;

/// Rayons (unités monde) des anneaux, mesurés depuis le point focal. `RADII[k]` est la
/// distance à partir de laquelle on passe du niveau `k` au niveau `k + 1`.
const RADII: [f32; MAX_LOD as usize] = [220.0, 620.0, 1820.0];

/// Demi-largeur relative de la **bande morte** de l'hystérésis. Un chunk pile sur une
/// frontière d'anneau oscillerait sinon entre deux niveaux et se ferait re-mailler en
/// boucle : on exige de passer nettement sous le rayon pour gagner en détail, et
/// nettement au-dessus pour en perdre. Principe du thermostat. 0.08 ⇒ le rayon 220 se
/// dédouble en 202 / 238.
const HYSTERESIS: f32 = 0.08;

/// Poids de la hauteur de caméra dans la distance qui pilote le LOD (cf. [`LodFocus`]).
/// `1.0` = distance euclidienne 3D honnête ; baisser pour atténuer l'effet de l'altitude.
const HEIGHT_WEIGHT: f32 = 1.0;

/// Distance **horizontale** (monde) du centre du chunk `coord` au point `focus`. Sert à
/// décrire la forme du monde chargé (le disque), pas à choisir le LOD — pour ça, cf.
/// [`LodFocus::distance`].
pub fn chunk_distance(coord: IVec2, focus: Vec2) -> f32 {
    let half = CHUNK_SIZE as f32 / 2.0;
    let cx = (coord.x * CHUNK_SIZE as i32) as f32 + half;
    let cy = (coord.y * CHUNK_SIZE as i32) as f32 + half;
    ((cx - focus.x).powi(2) + (cy - focus.y).powi(2)).sqrt()
}

/// Point de vue depuis lequel on juge du niveau de détail : la position horizontale de
/// la caméra **et sa hauteur** au-dessus du terrain.
///
/// ## Pourquoi la hauteur compte
/// Ce qui justifie un LOD grossier, c'est la petitesse d'un chunk **à l'écran**, laquelle
/// varie comme l'inverse de sa distance à l'œil — pas de sa distance au sol. Survoler le
/// terrain à 400 unités d'altitude éloigne tout autant qu'un recul de 400 unités : la
/// bonne mesure est donc simplement la distance euclidienne 3D œil → chunk.
///
/// On l'obtient en composant les deux :
///
/// ```text
/// d = √( d_horizontale² + (poids · hauteur)² )
/// ```
///
/// Conséquence recherchée : depuis un sommet de montagne, l'anneau LOD0 se resserre tout
/// seul et bien plus de chunks basculent en faible détail — alors que dans une vallée,
/// où l'on ne voit pas loin de toute façon, rien ne change.
#[derive(Clone, Copy, PartialEq)]
pub struct LodFocus {
    /// Position horizontale de la caméra (monde).
    pub pos: Vec2,
    /// Hauteur au-dessus de l'altitude de référence du terrain, **jamais négative**.
    pub height: f32,
}

impl LodFocus {
    /// Point focal depuis la position caméra. `reference_z` est l'altitude moyenne du
    /// relief (cf. `ChunkManager::mean_terrain_height`) : c'est le plan par rapport
    /// auquel on mesure « être haut ».
    ///
    /// Le `max(0.0)` traite le cas « caméra sous la référence » (fond de vallée, sous
    /// terre) comme une hauteur nulle : on y retombe sur la distance horizontale pure.
    /// Un creux ne doit pas faire grossir le LOD comme le ferait une altitude — d'en bas
    /// on voit *moins* loin, pas plus.
    pub fn new(camera_pos: Vec3, reference_z: f32) -> LodFocus {
        LodFocus {
            pos: camera_pos.truncate(),
            height: (camera_pos.z - reference_z).max(0.0),
        }
    }

    /// Distance œil → chunk qui pilote le LOD (horizontale composée avec la hauteur).
    #[inline]
    pub fn distance(self, coord: IVec2) -> f32 {
        let d = chunk_distance(coord, self.pos);
        let h = self.height * HEIGHT_WEIGHT;
        (d * d + h * h).sqrt()
    }

    /// Carré du déplacement du point focal depuis `other`, hauteur comprise. Au carré :
    /// c'est le test fait à **chaque frame**, aucune racine n'y a sa place.
    #[inline]
    pub fn moved_sq(self, other: LodFocus) -> f32 {
        self.pos.distance_squared(other.pos) + (self.height - other.height).powi(2)
    }
}

/// LOD **brut** d'un chunk sans mémoire de son état précédent : le nombre d'anneaux
/// franchis. Sert à l'initialisation, avant que l'hystérésis n'ait un état à relire.
pub fn static_lod(coord: IVec2, focus: LodFocus) -> u8 {
    let d = focus.distance(coord);
    RADII.iter().take_while(|&&r| d >= r).count() as u8
}

/// LOD **brut** avec hystérésis : même règle que [`static_lod`], mais chaque rayon est
/// décalé selon le niveau `current` déjà occupé par le chunk.
///
/// - le chunk a déjà franchi le rayon `k` (`current > k`) → pour revenir en deçà, il doit
///   descendre sous `RADII[k]·(1 − HYSTERESIS)` ;
/// - il ne l'a pas franchi → pour le franchir, il doit dépasser `RADII[k]·(1 + HYSTERESIS)`.
///
/// ⚠ `current` doit être le niveau **brut** du chunk, jamais celui d'après équilibrage
/// 2:1 — sinon l'hystérésis dérive (cf. [`grid::LodGrid`]).
pub fn hysteretic_lod(coord: IVec2, focus: LodFocus, current: u8) -> u8 {
    let d = focus.distance(coord);
    let mut lod = 0;
    for (k, &radius) in RADII.iter().enumerate() {
        let threshold = if current > k as u8 {
            radius * (1.0 - HYSTERESIS)
        } else {
            radius * (1.0 + HYSTERESIS)
        };
        if d < threshold {
            break;
        }
        lod = k as u8 + 1;
    }
    lod
}

/// Écart de LOD maximal toléré entre deux chunks **voisins**. La cellule de transition
/// du Transvoxel suppose un voisin de pas exactement **double** (sa face grossière a
/// 2×2 coins face à 3×3 échantillons fins) : un écart de 2 niveaux n'est pas coudable.
const MAX_LOD_STEP: u8 = 1;

// ─── Détection des faces de transition (Transvoxel) ──────────────────────────
//
// Deux chunks voisins à des LOD différents laissent une fente sur leur face commune.
// On la scelle par une **cellule de transition** (cf. [`transvoxel_tables`]) posée du
// côté HAUTE RÉSOLUTION : le chunk fin, celui qui a le plus de sommets à raccorder
// vers le bord grossier. Règle asymétrique — sur une face donnée, un seul des deux
// chunks (le plus fin) construit la transition : jamais les deux, jamais aucun. Chaque
// chunk est une colonne pleine hauteur tuilée en X/Y : ses seuls voisins sont
// horizontaux ⇒ 4 faces possibles (aucun voisin en Z).

/// Face horizontale d'un chunk, orientée vers son voisin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    NegX, // ouest
    PosX, // est
    NegY, // sud
    PosY, // nord
}

impl Face {
    /// Les 4 faces, dans l'ordre attendu par [`transition_faces`] pour `neighbor_lods`.
    pub const ALL: [Face; 4] = [Face::NegX, Face::PosX, Face::NegY, Face::PosY];

    /// Décalage (coordonnées chunk) vers le voisin situé de ce côté.
    pub fn offset(self) -> IVec2 {
        match self {
            Face::NegX => IVec2::new(-1, 0),
            Face::PosX => IVec2::new(1, 0),
            Face::NegY => IVec2::new(0, -1),
            Face::PosY => IVec2::new(0, 1),
        }
    }
}

/// Ensemble des faces d'un chunk portant une cellule de transition (bitmask, 1 bit par
/// [`Face`]). Le mailleur itère dessus pour savoir quels bords coudre.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct TransitionFaces(u8);

impl TransitionFaces {
    /// La face `f` porte-t-elle une transition ?
    #[inline]
    pub fn contains(self, f: Face) -> bool {
        self.0 & (1 << f as u8) != 0
    }

    /// Aucune face à coudre (cas courant : intérieur d'un anneau de LOD).
    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Itère les faces actives (consommé par le mailleur aux étapes suivantes).
    pub fn iter(self) -> impl Iterator<Item = Face> {
        Face::ALL.into_iter().filter(move |&f| self.contains(f))
    }

    #[inline]
    fn set(&mut self, f: Face) {
        self.0 |= 1 << f as u8;
    }
}

/// **Règle de détection** (pure) : une face porte une transition ssi le voisin de ce
/// côté est PLUS GROSSIER (`neighbor_lod > my_lod`). `neighbor_lods` suit l'ordre de
/// [`Face::ALL`]. Un voisin plus fin ou de même LOD ⇒ rien (si transition il faut,
/// c'est l'autre chunk, le plus fin, qui la construit). Un voisin absent est passé en
/// LOD 0 par l'appelant : jamais plus grossier ⇒ pas de fausse transition au bord du
/// monde chargé.
pub fn transition_faces(my_lod: u8, neighbor_lods: [u8; 4]) -> TransitionFaces {
    let mut faces = TransitionFaces::default();
    for (&f, &nlod) in Face::ALL.iter().zip(neighbor_lods.iter()) {
        if nlod > my_lod {
            faces.set(f);
        }
    }
    faces
}
