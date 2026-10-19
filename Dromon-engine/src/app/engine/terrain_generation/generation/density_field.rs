//! Champ de densité **3D** — la source de vérité du terrain (« données ≠ géométrie »).
//!
//! Le terrain est une fonction scalaire de l'espace : `sample(x,y,z)` > ISO_LEVEL =
//! matière, < = vide, == = surface. Le mailleur ne connaît QUE cette interface
//! (`sample` + `vertical_bounds`) et ignore comment la densité est produite → on peut
//! enrichir le terrain (grottes, surplombs, filons) sans toucher au mailleur.

use crate::app::engine::terrain_generation::chunk::{CHUNK_SIZE, Voxel};
use crate::profile;

use super::material::{MaterialQuery, MaterialTable, evaluate};
use super::world_sampler::WorldSampler;
use glam::{DVec2, IVec2, IVec3, Vec3};
use rustc_hash::FxHashMap;
use std::cell::Cell;

/// Pas du treillis global sur lequel est calculée la pente macro aux pas fins (cf.
/// [`DensityField::node_macro_gradient`]). Le rayon y est arrondi à un multiple de 4
/// (`lattice_macro_radius`) : les colonnes lues à ±r retombent sur le treillis.
const MACRO_LATTICE: i32 = 4;

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
    /// Altitude des colonnes (carte macro + fBm), côte et climat pour les matériaux.
    sampler: &'a WorldSampler,
    /// Matériaux (IDs et réglages des règles).
    materials: &'a MaterialTable,
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
    /// (coins de cube +1, pochoir des normales) et celui de la pente macro
    /// ([`DensityField::lattice_macro_radius`] : 12 aux pas fins, `+1` de marge ; aux pas
    /// grossiers, son rayon reste sous celui du pochoir des normales).
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
    /// rayon des normales) ; `step` est son pas d'échantillonnage (`1 << lod`) ;
    /// `max_height` le plafond du monde.
    pub fn new(
        sampler: &'a WorldSampler,
        materials: &'a MaterialTable,
        edits: FxHashMap<IVec3, f32>,
        coord: IVec2,
        apron: i32,
        step: i32,
        max_height: u32,
    ) -> DensityField<'a> {
        // Élargir ne coûte que de la mémoire : les colonnes sont calculées à la lecture.
        let macro_apron = materials.rules.slope.macro_radius.ceil() as i32 + 1;
        let apron = apron.max(macro_apron);
        let side = CHUNK_SIZE as i32 + 2 * apron + 1;
        let mut field = DensityField {
            sampler,
            materials,
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
        field.z_max = (mx.ceil() as i32).min(max_height as i32 - 2);
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
        profile!("colonne (relief)");
        let wx = (self.origin_x + gx - self.apron) as f64;
        let wy = (self.origin_y + gy - self.apron) as f64;
        let h = self.sampler.height(wx, wy) as f32;
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
    /// position exacte (cf. [`DensityField::volume_material`]).
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

    /// Pente macro au sommet posé à la fraction `t` de l'arête `a → b` : interpolation
    /// des pentes de ses deux extrémités, nœuds de la grille de pas `scale`.
    ///
    /// Même raison que pour les normales (`vertex_normal`) : mesurée au point exact, à
    /// ±12 voxels, la pente lisait 4 colonnes hors grille par sommet, sans partage dès
    /// que les sommets sont espacés (2,8 colonnes neuves par sommet à LOD 3, premier
    /// poste du chunk). Aux nœuds, avec un rayon multiple de `scale`, ses colonnes sont
    /// celles de la géométrie.
    pub fn edge_macro_gradient(&self, a: IVec3, b: IVec3, t: f32, scale: i32) -> DVec2 {
        let ga = self.node_macro_gradient(a.x, a.y, scale);
        let gb = self.node_macro_gradient(b.x, b.y, scale);
        ga.lerp(gb, t as f64)
    }

    /// Pente macro au nœud `(wx, wy)` d'une grille de pas `scale`.
    ///
    /// Aux pas fins (< [`MACRO_LATTICE`]), interpolée bilinéairement entre les nœuds du
    /// treillis global de pas `MACRO_LATTICE`. Calculée à chaque nœud, à ±12 voxels, elle
    /// lisait toute la bande d'apron autour du chunk : 2 730 colonnes de fBm sur 7 345 à
    /// LOD 0 (21 % du chunk), pour une pente lissée sur 24 voxels de toute façon. Sur le
    /// treillis, ces colonnes tombent toutes sur la grille de pas 4 (~16× moins).
    ///
    /// Treillis **global** : deux chunks voisins lisent la même pente sur leur bord commun
    /// (matériaux continus). Aux pas ≥ 4, les nœuds y sont déjà : calcul direct, valeurs
    /// identiques à celles qu'interpole un chunk plus fin.
    fn node_macro_gradient(&self, wx: i32, wy: i32, scale: i32) -> DVec2 {
        if scale >= MACRO_LATTICE {
            return self.lattice_macro_gradient(wx, wy, scale);
        }
        let p = MACRO_LATTICE;
        let (x0, y0) = (wx.div_euclid(p) * p, wy.div_euclid(p) * p);
        let tx = (wx - x0) as f64 / p as f64;
        let ty = (wy - y0) as f64 / p as f64;
        // Nœud suivant lu seulement s'il pèse : sur le bord haut du chunk (x = 64), il
        // tomberait hors de l'apron.
        let x1 = if tx > 0.0 { x0 + p } else { x0 };
        let y1 = if ty > 0.0 { y0 + p } else { y0 };
        let g = |x: i32, y: i32| self.lattice_macro_gradient(x, y, p);
        let g0 = g(x0, y0).lerp(g(x1, y0), tx);
        let g1 = g(x0, y1).lerp(g(x1, y1), tx);
        g0.lerp(g1, ty)
    }

    /// Gradient `∇h` **moyen** du relief au nœud `(wx, wy)`, par différences centrées
    /// d'écart `2r` ([`DensityField::lattice_macro_radius`]). Une différence de hauteurs
    /// sur `[x−r, x+r]` vaut la moyenne de la pente locale sur ce segment : les bosses
    /// plus étroites que `2r` s'annulent, seules les grandes pentes restent. Lue dans la
    /// grille mémoïsée, pas au fBm.
    fn lattice_macro_gradient(&self, wx: i32, wy: i32, scale: i32) -> DVec2 {
        let r = self.lattice_macro_radius(scale);
        let h = |x: i32, y: i32| self.relief(x, y) as f64;
        let span = (2 * r) as f64;
        DVec2::new(
            (h(wx + r, wy) - h(wx - r, wy)) / span,
            (h(wx, wy + r) - h(wx, wy - r)) / span,
        )
    }

    /// `slope.macro_radius` arrondi au multiple de `scale` le plus proche (au moins un
    /// pas) : 12 de LOD 0 à 2, 16 à LOD 3 et 4. Écart de rayon invisible à ces
    /// distances.
    fn lattice_macro_radius(&self, scale: i32) -> i32 {
        let r = self.materials.rules.slope.macro_radius;
        ((r / scale as f64).round() as i32).max(1) * scale
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
    /// `- cave_strength * self.sampler.cave_noise(wx, wy, wz)`.
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

    /// Relief min et max aux 4 coins de la colonne de cubes MC de coin `(wx, wy)` et de
    /// côté `step`. Hors de cet intervalle en z, la densité `relief − z` (linéaire en z)
    /// est de même signe aux 8 coins d'un cube : pas de surface. `None` si la région porte
    /// des édits, qui cassent ce raisonnement.
    #[inline]
    pub fn cube_column_relief(&self, wx: i32, wy: i32, step: i32) -> Option<(f32, f32)> {
        if !self.edits.is_empty() {
            return None;
        }
        let h = [
            self.relief(wx, wy),
            self.relief(wx + step, wy),
            self.relief(wx, wy + step),
            self.relief(wx + step, wy + step),
        ];
        Some((
            h.iter().copied().fold(f32::INFINITY, f32::min),
            h.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        ))
    }

    /// Matériaux d'un sommet d'**iso-surface** de normale `normal` et de pente macro
    /// `gradient` (cf. [`DensityField::edge_macro_gradient`]) : couverture de surface
    /// selon son climat et sa pente.
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
    pub fn surface_material(&self, p: Vec3, normal: Vec3, gradient: DVec2) -> Voxel {
        profile!("matériau surface");
        let (wx, wy) = (p.x as f64, p.y as f64);
        // Jitter unitaire, mis à l'échelle par chaque frontière.
        let noise = &self.materials.noise;
        let jitter = noise.jitter(wx, wy);
        // Altitude au-dessus de la mer : référence des frontières littorales.
        let alt = p.z as f64 - self.sampler.sea_level();
        let shore = &self.materials.rules.shore;
        let climate = {
            profile!("climat");
            self.sampler.climate(wx, wy, p.z as f64, gradient)
        };
        evaluate(&MaterialQuery {
            ids: &self.materials.ids,
            rules: &self.materials.rules,
            pos: p,
            depth: 0.0,
            climate,
            jitter,
            shore_d: self.sampler.coast_distance(wx, wy) + jitter * shore.beach_jitter,
            sea_depth: -alt + jitter * shore.seabed_jitter,
            normal: Some(normal),
            // ‖∇h‖ = tan θ → cos θ = 1 / √(1 + tan² θ), comparable à `normal.z`.
            macro_up: Some(1.0 / (1.0 + gradient.length_squared()).sqrt()),
            patch_noise: Some(noise.dirt_patch(wx, wy)),
            geology_noise: Some(noise.geology(wx, wy)),
        })
    }

    /// Matériaux d'un point **de volume** en `p` (coordonnées monde flottantes) : à sa
    /// profondeur réelle sous le relief. Réservés aux parois de bordure et au fond,
    /// dont la géométrie coupe le terrain et doit en montrer les strates.
    ///
    /// Leurs sommets sont posés aux **nœuds** de la grille du chunk (pas `step`), où
    /// l'interpolation retombe exactement sur la grille pré-échantillonnée : la
    /// profondeur y est exacte quel que soit le LOD, et la pente macro s'y lit comme
    /// celle de la surface. Pour un sommet d'iso-surface, prendre
    /// [`DensityField::surface_material`].
    pub fn volume_material(&self, p: Vec3, step: i32) -> Voxel {
        profile!("matériau volume");
        let (wx, wy) = (p.x as f64, p.y as f64);
        let surface = self.relief_interp(wx, wy) as f64;
        let jitter = self.materials.noise.jitter(wx, wy);
        let alt = surface - self.sampler.sea_level();
        let shore = &self.materials.rules.shore;
        let gradient = self.node_macro_gradient(p.x.round() as i32, p.y.round() as i32, step);
        evaluate(&MaterialQuery {
            ids: &self.materials.ids,
            rules: &self.materials.rules,
            pos: p,
            depth: surface - p.z as f64,
            // Climat de la surface au-dessus : c'est elle que couvre la neige.
            climate: self.sampler.climate(wx, wy, surface, gradient),
            jitter,
            shore_d: self.sampler.coast_distance(wx, wy) + jitter * shore.beach_jitter,
            sea_depth: -alt + jitter * shore.seabed_jitter,
            normal: None,
            macro_up: None,
            patch_noise: None,
            geology_noise: None,
        })
    }

    /// ID du matériau de debug (magenta), pour les teintes de `debug.ron`.
    pub fn debug_material(&self) -> u16 {
        self.materials.ids.debug
    }
}
