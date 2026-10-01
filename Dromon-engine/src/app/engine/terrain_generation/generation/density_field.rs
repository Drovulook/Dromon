//! Champ de densité **3D** — la source de vérité du terrain (« données ≠ géométrie »).
//!
//! Le terrain est une fonction scalaire de l'espace : `sample(x,y,z)` > ISO_LEVEL =
//! matière, < = vide, == = surface. Le mailleur ne connaît QUE cette interface
//! (`sample` + `vertical_bounds`) et ignore comment la densité est produite → on peut
//! enrichir le terrain (grottes, surplombs, filons) sans toucher au mailleur.

use crate::app::engine::terrain_generation::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, Voxel};
use crate::profile;

use super::height_field::HeightField;
use super::material::{
    MACRO_SLOPE_RADIUS, MaterialQuery, SURFACE_JITTER_AMP, VOLUME_JITTER_AMP, evaluate,
    material_color,
};
use glam::{IVec2, IVec3, Vec3};
use rustc_hash::FxHashMap;
use std::cell::Cell;

/// Marge de grille nécessaire à [`DensityField::macro_up`] : un sommet au bord du chunk
/// lit le relief à `MACRO_SLOPE_RADIUS` au-delà, arrondi à la colonne entière la plus
/// proche (`+1` couvre l'arrondi vers le haut).
const MACRO_APRON: i32 = MACRO_SLOPE_RADIUS as i32 + 1;

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
/// Le relief ne dépendant que de `(x, y)`, on le **mémoïse** par colonne sur la région
/// (grille `relief`, une éval. de fBm par colonne) : `sample` redevient de
/// l'arithmétique, sans hachage.
///
/// Mémoïsé **à la demande**, pas pré-calculé : un chunk LOD `k` ne lit qu'une colonne
/// sur `2^k` par axe (plus le voisinage des sommets pour les normales). Tout calculer
/// d'avance faisait payer à un chunk LOD 3 le relief d'un LOD 0 (7,3 ms mesurées).
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
    /// Marge de la grille `relief` autour du chunk : couvre le débordement du mailleur
    /// (coins de cube +1, stencil des normales ±rayon) et celui de
    /// [`DensityField::macro_up`] (cf. [`MACRO_APRON`]).
    apron: i32,
    /// Côté de la grille `relief` : `CHUNK_SIZE + 2·apron + 1`.
    side: i32,
    /// Relief mémoïsé, indexé `gx * side + gy` (apron compris). `NaN` = pas encore
    /// calculé. `Cell` : rempli depuis `&self`, le champ ne quittant jamais son thread.
    relief: Vec<Cell<f32>>,
    /// Tranche verticale utile (cf. [`DensityField::vertical_bounds`]).
    z_min: i32,
    z_max: i32,
}

impl<'a> DensityField<'a> {
    /// Prépare le champ sur la région du chunk `coord` et calcule ses bornes verticales.
    /// `apron` doit couvrir tout ce que le mailleur échantillonne au-delà des bords (le
    /// rayon des normales) ; `step` est son pas d'échantillonnage (`1 << lod`).
    pub fn new(
        height: &'a HeightField,
        edits: FxHashMap<IVec3, f32>,
        coord: IVec2,
        apron: i32,
        step: i32,
    ) -> DensityField<'a> {
        // Élargir ne coûte que de la mémoire : les colonnes sont calculées à la lecture.
        let apron = apron.max(MACRO_APRON);
        let side = CHUNK_SIZE as i32 + 2 * apron + 1;
        let mut field = DensityField {
            height,
            edits,
            origin_x: coord.x * CHUNK_SIZE as i32,
            origin_y: coord.y * CHUNK_SIZE as i32,
            apron,
            side,
            relief: vec![Cell::new(f32::NAN); (side * side) as usize],
            z_min: 0,
            z_max: 0,
        };

        // Bornes verticales, dérivées du champ lui-même : plus petit / plus grand relief
        // sur les colonnes que le mailleur échantillonne à ce pas (MC et transitions
        // tombent sur les multiples de `step`, de 0 à CHUNK_SIZE inclus). Sous `z_min`
        // le champ est plein partout, au-dessus de `z_max` vide partout → aucune surface
        // à mailler en dehors de `[z_min, z_max]`.
        let mut mn = f32::INFINITY;
        let mut mx = f32::NEG_INFINITY;
        for x in (0..=CHUNK_SIZE as i32).step_by(step as usize) {
            for y in (0..=CHUNK_SIZE as i32).step_by(step as usize) {
                let h = field.column(x + apron, y + apron);
                mn = mn.min(h);
                mx = mx.max(h);
            }
        }
        field.z_min = (mn.floor() as i32 - 1).max(0);
        field.z_max = (mx.ceil() as i32).min(CHUNK_HEIGHT as i32 - 2);
        field
    }

    /// Relief de la case `(gx, gy)` de la grille, calculé au premier accès.
    #[inline]
    fn column(&self, gx: i32, gy: i32) -> f32 {
        let cell = &self.relief[(gx * self.side + gy) as usize];
        let h = cell.get();
        if !h.is_nan() {
            return h;
        }
        let wx = (self.origin_x + gx - self.apron) as f64;
        let wy = (self.origin_y + gy - self.apron) as f64;
        let h = self.height.height(wx, wy) as f32;
        cell.set(h);
        h
    }

    /// Relief (sommet de la couche pleine) à la colonne monde `(wx, wy)`. Détail interne — suppose la colonne dans la région (garanti pour
    /// tout ce que le mailleur échantillonne, cf. `apron`).
    #[inline]
    fn relief(&self, wx: i32, wy: i32) -> f32 {
        let gx = wx - self.origin_x + self.apron;
        let gy = wy - self.origin_y + self.apron;
        debug_assert!(
            gx >= 0 && gx < self.side && gy >= 0 && gy < self.side,
            "échantillon hors de la région pré-échantillonnée (apron trop petit ?)"
        );
        self.column(gx, gy)
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
        let at = |gx: i32, gy: i32| self.column(gx, gy);
        let h0 = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
        let h1 = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
        h0 + (h1 - h0) * ty
    }

    /// Cosinus de la pente **moyenne** du relief sur un voisinage de rayon
    /// [`MACRO_SLOPE_RADIUS`] autour de `(wx, wy)`, par différences centrées d'écart
    /// `2r`. Une différence de hauteurs sur `[x−r, x+r]` vaut la moyenne de la pente
    /// locale sur ce segment : les bosses plus étroites que `2r` s'annulent, seules les
    /// grandes pentes restent.
    ///
    /// Lue dans la grille mémoïsée plutôt qu'au fBm : les sommets voisins retombent sur
    /// les mêmes colonnes. 4 `height()` par sommet coûtaient la moitié d'un chunk LOD 0.
    ///
    /// Colonne **arrondie**, pas interpolée : l'interpolation lit 4 colonnes par point,
    /// soit 16 par sommet — ruineux aux LOD grossiers, où les sommets espacés partagent
    /// peu de colonnes. 0,5 voxel d'écart sur une pente mesurée à 12 ne se voit pas.
    fn macro_up(&self, wx: f64, wy: f64) -> f64 {
        let r = MACRO_SLOPE_RADIUS;
        let h = |x: f64, y: f64| self.relief(x.round() as i32, y.round() as i32) as f64;
        let gx = (h(wx + r, wy) - h(wx - r, wy)) / (2.0 * r);
        let gy = (h(wx, wy + r) - h(wx, wy - r)) / (2.0 * r);
        // ‖∇h‖ = tan θ → cos θ = 1 / √(1 + tan² θ), comparable à `normal.z`.
        1.0 / (1.0 + gx * gx + gy * gy).sqrt()
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

    /// Couleur d'un sommet d'**iso-surface** de normale `normal` : matériau de surface à
    /// son altitude et selon sa pente.
    ///
    /// Sa profondeur sous le toit vaut zéro *par construction* — on ne la mesure donc
    /// pas. La mesurer serait même faux : le mailleur pose le sommet sur la corde
    /// joignant deux échantillons distants de `1 << lod`, alors que [`relief_interp`]
    /// rend le relief au pas 1. Sur une portion convexe l'écart entre les deux croît
    /// comme le pas, dépasse `SURFACE_DEPTH` dès le LOD 2 et fait basculer le sommet
    /// en terre — d'où les étoiles marron isolées au loin (un seul sommet fautif
    /// colorie tout son 1-ring par interpolation de Gouraud).
    ///
    /// L'altitude testée vient de `p.z` lui-même : sur l'iso-surface c'est le relief,
    /// en moins cher et sans écart de résolution.
    ///
    /// [`relief_interp`]: DensityField::relief_interp
    pub fn surface_color(&self, p: Vec3, normal: Vec3) -> Vec3 {
        let (wx, wy) = (p.x as f64, p.y as f64);
        let jitter = self.height.material_jitter(wx, wy, SURFACE_JITTER_AMP);
        blend(evaluate(&MaterialQuery {
            pos: p,
            depth: 0.0,
            mat_alt: p.z as f64 + jitter,
            normal: Some(normal),
            macro_up: Some(self.macro_up(wx, wy)),
            patch_noise: Some(self.height.dirt_patch_noise(wx, wy)),
        }))
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
        let jitter = self.height.material_jitter(p.x as f64, p.y as f64, VOLUME_JITTER_AMP);
        blend(evaluate(&MaterialQuery {
            pos: p,
            depth: surface - p.z as f64,
            mat_alt: surface + jitter,
            normal: None,
            macro_up: None,
            patch_noise: None,
        }))
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
