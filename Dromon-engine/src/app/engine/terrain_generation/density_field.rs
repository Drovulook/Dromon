//! Champ de densité **3D** — la source de vérité du terrain (« données ≠ géométrie »).
//!
//! Le terrain est une fonction scalaire de l'espace : `sample(x,y,z)` > ISO_LEVEL =
//! matière, < = vide, == = surface. Le mailleur ne connaît QUE cette interface
//! (`sample` + `vertical_bounds`) et ignore comment la densité est produite → on peut
//! enrichir le terrain (grottes, surplombs, filons) sans toucher au mailleur.

use super::chunk::{CHUNK_HEIGHT, CHUNK_SIZE};
use super::height_field::HeightField;
use super::material::{classify_solid, material_color};
use glam::{IVec2, IVec3, Vec3};
use std::collections::HashMap;

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
    /// Overlay épars des densités **modifiées** (creuser/remblayer), prioritaire.
    edits: &'a HashMap<IVec3, f32>,
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
        edits: &'a HashMap<IVec3, f32>,
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
        // Court-circuit tant qu'aucune édition : évite de hacher une clé pour rien
        // (chemin ultra-chaud, appelé des millions de fois au maillage).
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

    /// Couleur du point de surface en `(wx, wy, wz)` : mélange des matériaux dominants,
    /// choisis par la profondeur sous le relief et l'altitude (cf.
    /// [`super::material::classify_solid`]). Dépend du champ, donc vit ici.
    pub fn color(&self, wx: i32, wy: i32, wz: i32) -> Vec3 {
        let surface = self.relief(wx, wy) as f64;
        let depth = surface - wz as f64;
        let mat_alt = surface + self.height.material_jitter(wx as f64, wy as f64);
        let voxel = classify_solid(depth, mat_alt);

        let mut color = Vec3::ZERO;
        for i in 0..4 {
            let weight = voxel.weights[i] as f32 / 255.0;
            color += material_color(voxel.materials[i]) * weight;
        }
        color
    }
}
