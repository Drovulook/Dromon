//! Champ de densité **3D** — la source de vérité du terrain (« données ≠ géométrie »).
//!
//! Le terrain est une fonction scalaire de l'espace : `sample(x,y,z)` > ISO_LEVEL =
//! matière, < = vide, == = surface. Le mailleur ne connaît QUE cette interface
//! (`sample` + `vertical_bounds`) et ignore comment la densité est produite → on peut
//! enrichir le terrain (grottes, surplombs, filons) sans toucher au mailleur.

use crate::app::engine::terrain_generation::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, Voxel};
use crate::profile;

use super::height_field::HeightField;
use super::material::{classify_solid, material_color};
use glam::{IVec2, IVec3, Vec3};
use rustc_hash::FxHashMap;

/// Vue échantillonnable du champ 3D sur la **région d'un chunk** (plus une marge
/// « apron » pour les normales). Construite le temps d'un maillage.
///
/// ## Pensé 3D, mais aujourd'hui limité au relief
/// Actuellement `density = relief(x,y) − z`. Rien ici ne suppose que ça se réduise à
/// une hauteur : pour ajouter des **grottes**, on soustraira un bruit 3D dans
/// [`DensityField::sample`] et on abaissera la borne basse de
/// [`DensityField::vertical_bounds`] — Marching Cubes maillera le vide sans autre
/// changement.
///
/// ## Optimisation interne (invisible au mailleur)
/// Le relief ne dépendant que de `(x, y)`, on le **pré-échantillonne** une fois par
/// colonne sur la région (grille `relief`, une éval. de fBm par colonne) : `sample`
/// redevient de l'arithmétique, sans hachage. Pur détail d'implémentation.
pub struct DensityField<'a> {
    /// Générateur du relief (fBm 2D). Sert aussi au choix des matériaux (couleur).
    height: &'a HeightField,
    /// Édits couvrant la région (chunk + apron), en coordonnées monde. Fusionnés une
    /// fois à la construction par [`TerrainSnapshot::density_field`] : ne contient que
    /// ce que cette région peut échantillonner, donc vide dans l'immense majorité des
    /// chunks — ce qui garde le court-circuit de [`DensityField::sample`] armé.
    ///
    /// [`TerrainSnapshot::density_field`]: super::super::chunk::TerrainSnapshot::density_field
    edits: FxHashMap<IVec3, f32>,
    /// Coin `(x, y)` monde du chunk (avant apron).
    origin_x: i32,
    origin_y: i32,
    /// Marge de pré-échantillonnage autour du chunk (couvre le débordement du mailleur :
    /// coins de cube +1 et stencil des normales ±rayon).
    apron: i32,
    /// Côté de la grille `relief` : `CHUNK_SIZE + 2·apron + 1`.
    side: i32,
    /// Relief pré-échantillonné, indexé `gx * side + gy` (apron compris).
    relief: Vec<f32>,
    /// Tranche verticale utile (cf. [`DensityField::vertical_bounds`]).
    z_min: i32,
    z_max: i32,
}

impl<'a> DensityField<'a> {
    /// Prépare le champ sur la région du chunk `coord` : pré-échantillonne le relief
    /// (chunk + `apron`) et calcule les bornes verticales. `apron` doit couvrir tout
    /// ce que le mailleur échantillonne au-delà des bords (le rayon des normales).
    pub fn new(
        height: &'a HeightField,
        edits: FxHashMap<IVec3, f32>,
        coord: IVec2,
        apron: i32,
    ) -> DensityField<'a> {
        let origin_x = coord.x * CHUNK_SIZE as i32;
        let origin_y = coord.y * CHUNK_SIZE as i32;
        let side = CHUNK_SIZE as i32 + 2 * apron + 1;

        // Une éval. de fBm par colonne, réutilisée pour tous les z : c'est ce qui rend
        // `sample` bon marché dans la boucle chaude.
        let mut relief = vec![0.0f32; (side * side) as usize];
        for gx in 0..side {
            for gy in 0..side {
                let wx = (origin_x + gx - apron) as f64;
                let wy = (origin_y + gy - apron) as f64;
                relief[(gx * side + gy) as usize] = height.height(wx, wy) as f32;
            }
        }

        // Bornes verticales, dérivées du champ lui-même : plus petit / plus grand
        // relief SUR LES COLONNES DU CHUNK (les coins de cube vont de 0 à CHUNK_SIZE
        // inclus). Sous `z_min` le champ est plein partout, au-dessus de `z_max` il est
        // vide partout → aucune surface à mailler en dehors de `[z_min, z_max]`.
        let mut mn = f32::INFINITY;
        let mut mx = f32::NEG_INFINITY;
        for gx in apron..=apron + CHUNK_SIZE as i32 {
            for gy in apron..=apron + CHUNK_SIZE as i32 {
                let h = relief[(gx * side + gy) as usize];
                mn = mn.min(h);
                mx = mx.max(h);
            }
        }
        let z_min = (mn.floor() as i32 - 1).max(0);
        let z_max = (mx.ceil() as i32).min(CHUNK_HEIGHT as i32 - 2);

        DensityField {
            height,
            edits,
            origin_x,
            origin_y,
            apron,
            side,
            relief,
            z_min,
            z_max,
        }
    }

    /// Relief (sommet de la couche pleine) pré-échantillonné à la colonne monde
    /// `(wx, wy)`. Détail interne — suppose la colonne dans la région (garanti pour
    /// tout ce que le mailleur échantillonne, cf. `apron`).
    #[inline]
    fn relief(&self, wx: i32, wy: i32) -> f32 {
        let gx = wx - self.origin_x + self.apron;
        let gy = wy - self.origin_y + self.apron;
        debug_assert!(
            gx >= 0 && gx < self.side && gy >= 0 && gy < self.side,
            "échantillon hors de la région pré-échantillonnée (apron trop petit ?)"
        );
        self.relief[(gx * self.side + gy) as usize]
    }

    /// Relief interpolé (bilinéaire) à la colonne monde **flottante** `(wx, wy)` :
    /// [`DensityField::relief`] accepte seulement des entiers. Aux coins entiers il
    /// redonne exactement la grille. Sert à mesurer la profondeur d'un sommet à sa
    /// position exacte (cf. [`DensityField::volume_color`]).
    #[inline]
    fn relief_interp(&self, wx: f64, wy: f64) -> f32 {
        let fx = wx - self.origin_x as f64 + self.apron as f64;
        let fy = wy - self.origin_y as f64 + self.apron as f64;
        // Coin bas-gauche de la maille + fraction ; indices bornés à la grille (sûreté).
        let x0 = (fx.floor() as i32).clamp(0, self.side - 1);
        let y0 = (fy.floor() as i32).clamp(0, self.side - 1);
        let x1 = (x0 + 1).min(self.side - 1);
        let y1 = (y0 + 1).min(self.side - 1);
        let tx = (fx - fx.floor()) as f32;
        let ty = (fy - fy.floor()) as f32;
        let at = |gx: i32, gy: i32| self.relief[(gx * self.side + gy) as usize];
        let h0 = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
        let h1 = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
        h0 + (h1 - h0) * ty
    }

    /// Altitude du **toit** du terrain (sommet de la couche pleine) à la colonne
    /// `(wx, wy)` — c'est le relief. Sert au mailleur pour poser le haut des parois de
    /// bordure. (Avec des grottes, ça restera le toit ; les cavités seront ailleurs.)
    #[inline]
    pub fn surface_z(&self, wx: i32, wy: i32) -> f32 {
        self.relief(wx, wy)
    }

    /// **Densité signée 3D** en `(wx, wy, wz)`. C'est LE point d'extension du terrain :
    /// c'est ici — et seulement ici — qu'on écrit la physique du champ. Les **édits**
    /// (creuser/remblayer) priment sur le procédural.
    ///
    /// Terrain de base : `relief(x,y) − z`, linéaire en z (surface fractionnaire
    /// retrouvée par interpolation → pas de marches). **Grottes (à venir)** :
    /// soustraire un bruit 3D, p.ex.
    /// `- cave_strength * self.height.cave_noise(wx, wy, wz)`.
    #[inline]
    pub fn sample(&self, wx: i32, wy: i32, wz: i32) -> f32 {
        // Court-circuit tant qu'aucune édition **dans cette région** : évite de hacher
        // une clé pour rien sur un chemin appelé ~1,3 million de fois par chunk. Le test
        // était autrefois global — un seul trou creusé dans le monde le désarmait
        // partout, et rechargeait tout le maillage d'un hachage par échantillon.
        if !self.edits.is_empty() {
            if let Some(&d) = self.edits.get(&IVec3::new(wx, wy, wz)) {
                return d;
            }
        }
        self.relief(wx, wy) - wz as f32
    }

    /// Tranche verticale `[z_min, z_max]` (inclus) que le mailleur doit parcourir :
    /// hors d'elle, le champ est **uniforme** (plein en dessous, vide au-dessus) donc
    /// sans surface. Autrement dit, l'étendue en z où la densité peut basculer entre
    /// plein et vide, côté chunk.
    ///
    /// Ces bornes sont dérivées du champ (ici, du relief) : elles se **généralisent**
    /// naturellement. Avec des grottes, du vide apparaîtra en profondeur → il faudra
    /// abaisser `z_min` jusqu'au plancher des grottes (la borne haute reste valable :
    /// au-dessus du relief, tout est air).
    #[inline]
    pub fn vertical_bounds(&self) -> (i32, i32) {
        (self.z_min, self.z_max)
    }

    /// Couleur d'un sommet d'**iso-surface** : le biome de surface à son altitude.
    ///
    /// Sa profondeur sous le toit vaut zéro *par construction* — on ne la mesure donc
    /// pas. La mesurer serait même faux : le mailleur pose le sommet sur la corde
    /// joignant deux échantillons distants de `1 << lod`, alors que [`relief_interp`]
    /// rend le relief au pas 1. Sur une portion convexe l'écart entre les deux croît
    /// comme le pas, dépasse `SURFACE_DEPTH` dès le LOD 2 et fait basculer le sommet
    /// en terre — d'où les étoiles marron isolées au loin (un seul sommet fautif
    /// colorie tout son 1-ring par interpolation de Gouraud).
    ///
    /// L'altitude de biome vient de `p.z` lui-même : sur l'iso-surface c'est le relief,
    /// en moins cher et sans écart de résolution.
    ///
    /// [`relief_interp`]: DensityField::relief_interp
    pub fn surface_color(&self, p: Vec3) -> Vec3 {
        let mat_alt = p.z as f64 + self.height.material_jitter(p.x as f64, p.y as f64, 20.0);
        blend(classify_solid(0.0, mat_alt))
    }

    /// Couleur d'un point **de volume** en `p` (coordonnées monde flottantes) : matériau
    /// à sa profondeur réelle sous le relief. Réservée aux parois de bordure et au fond,
    /// dont la géométrie coupe le terrain et doit en montrer les strates.
    ///
    /// Leurs sommets sont posés à des colonnes **entières**, où l'interpolation retombe
    /// exactement sur la grille pré-échantillonnée : la profondeur y est exacte quel que
    /// soit le LOD. Pour un sommet d'iso-surface, prendre [`DensityField::surface_color`].
    pub fn volume_color(&self, p: Vec3) -> Vec3 {
        let surface = self.relief_interp(p.x as f64, p.y as f64) as f64;
        let depth = surface - p.z as f64;
        let mat_alt = surface + self.height.material_jitter(p.x as f64, p.y as f64, 20.0);
        blend(classify_solid(depth, mat_alt))
    }
}

/// Albédo d'un descripteur de matière : ses matériaux, pondérés par leurs poids.
fn blend(voxel: Voxel) -> Vec3 {
    let mut color = Vec3::ZERO;
    for i in 0..4 {
        color += material_color(voxel.materials[i]) * (voxel.weights[i] as f32 / 255.0);
    }
    color
}
