pub use super::chunk::{CHUNK_HEIGHT, CHUNK_SIZE, Chunk, ISO_LEVEL, Voxel};
pub use super::height_field::{HeightField, HeightParams};

use super::material::{classify_solid, material_color};
use glam::{IVec2, Vec3};
use std::collections::HashMap;

/// Paramètres de génération du relief : graine + forme du champ d'altitude.
pub struct GenParams {
    pub seed: u32,
    /// Réglages du fBm érodé (cf. [`HeightParams`]).
    pub height: HeightParams,
}

impl Default for GenParams {
    fn default() -> Self {
        GenParams {
            seed: 0,
            height: HeightParams::default(),
        }
    }
}

/// Détient les chunks générés et le champ d'altitude. Point d'entrée de la
/// génération procédurale du terrain.
pub struct ChunkManager {
    chunks: HashMap<IVec2, Chunk>,
    height: HeightField,
    params: GenParams,
}

impl ChunkManager {
    pub fn new(params: GenParams) -> ChunkManager {
        let height = HeightField::new(params.seed, params.height);
        ChunkManager {
            chunks: HashMap::new(),
            height,
            params,
        }
    }

    /// Génère (ou régénère) le chunk à la coordonnée donnée, le stocke et en
    /// renvoie une référence.
    ///
    /// On remplit un **champ de densité** : pour chaque colonne `(x, y)`, le
    /// bruit donne une hauteur de surface *continue* `surface_f`, et chaque voxel
    /// reçoit `density = surface_f − z`. La densité vaut donc 0 pile à la surface,
    /// est positive dessous (matière) et négative dessus (air) — et surtout elle
    /// est **linéaire en z**, ce qui permet à Surface Nets de retrouver la
    /// hauteur fractionnaire exacte par interpolation (surface lisse, sans
    /// marches). Les matériaux sont posés selon la profondeur sous la surface.
    pub fn generate_chunk(&mut self, coord: IVec2) -> &Chunk {
        let mut chunk = Chunk::new(coord);

        // Origine du chunk en coordonnées voxel « monde ».
        let origin_x = coord.x * CHUNK_SIZE as i32;
        let origin_y = coord.y * CHUNK_SIZE as i32;

        for lx in 0..CHUNK_SIZE {
            for ly in 0..CHUNK_SIZE {
                let wx = (origin_x + lx as i32) as f64;
                let wy = (origin_y + ly as i32) as f64;

                // Altitude continue de la colonne (fBm érodé, cf. `HeightField`).
                // Mise en cache dans le chunk : le mailleur la relira (position +
                // normales) sans recalculer le fBm. C'est le seul endroit où
                // `height()` est évalué pour ce chunk.
                let surface_f = self.height.height(wx, wy);
                chunk.set_column_height(lx, ly, surface_f as f32);

                // Altitude « bruitée » servant UNIQUEMENT au choix du matériau de
                // surface : la perturbation casse les frontières horizontales
                // rectilignes (sable/herbe/neige) sans toucher à la géométrie, qui
                // reste calée sur `surface_f`. Cf. `HeightField::material_jitter`.
                let mat_alt = surface_f + self.height.material_jitter(wx, wy);

                for z in 0..CHUNK_HEIGHT {
                    // depth > 0 sous la surface, < 0 au-dessus.
                    let depth = surface_f - z as f64;
                    let density = depth as f32;

                    // Air au-dessus de l'iso-surface ; sinon, matériau choisi par
                    // profondeur et altitude (cf. `material::classify_solid`), avec
                    // mélange continu de deux biomes près des bordures.
                    let voxel = if density < ISO_LEVEL {
                        Voxel::AIR
                    } else {
                        classify_solid(density, depth, mat_alt)
                    };
                    // On stocke même les voxels d'air : leur densité (négative)
                    // sert à Surface Nets pour interpoler la surface.
                    chunk.set_voxel(lx, ly, z, voxel);
                }
            }
        }

        self.chunks.insert(coord, chunk);
        &self.chunks[&coord]
    }

    /// Chunk déjà généré à cette coordonnée, le cas échéant.
    pub fn get_chunk(&self, coord: IVec2) -> Option<&Chunk> {
        self.chunks.get(&coord)
    }

    /// Densité à une position **monde** `(wx, wy, wz)`. Travaille en coordonnées
    /// monde pour que le mailleur interroge de façon transparente les chunks
    /// **voisins** au niveau des bords — c'est ce qui rend la couture continue.
    ///
    /// Hors monde : sous le sol (`wz < 0`) = plein (ferme le bas), au-dessus du
    /// plafond ou dans un chunk non généré = air.
    pub fn density_at(&self, wx: i32, wy: i32, wz: i32) -> f32 {
        if wz < 0 {
            return 1.0;
        }
        if wz as usize >= CHUNK_HEIGHT {
            return -1.0;
        }
        let size = CHUNK_SIZE as i32;
        // div_euclid / rem_euclid : division « plancher » correcte pour les
        // coordonnées négatives (contrairement à `/` et `%` qui tronquent).
        let coord = IVec2::new(wx.div_euclid(size), wy.div_euclid(size));
        match self.chunks.get(&coord) {
            Some(chunk) => {
                let lx = wx.rem_euclid(size) as usize;
                let ly = wy.rem_euclid(size) as usize;
                chunk.density(lx, ly, wz as usize)
            }
            None => -1.0,
        }
    }

    /// Couleur d'un sommet du mesh à une position **monde**, obtenue en mélangeant
    /// les couleurs des (jusqu'à) 4 matériaux dominants du voxel, pondérées par
    /// leurs poids `[0, 255]`. Renvoie noir hors chunk généré.
    ///
    /// Tant qu'un voxel ne porte qu'un seul matériau (`weights = [255,0,0,0]`),
    /// la somme se réduit à la couleur pure de ce matériau ; le dégradé entre
    /// zones (herbe↔neige…) vient alors de l'interpolation du rasterizer entre
    /// les sommets d'un triangle. Le mélange par poids prendra tout son sens avec
    /// le futur splatting top-K.
    pub fn color_at(&self, wx: i32, wy: i32, wz: i32) -> Vec3 {
        // Filet de sécurité : on borne `wz` dans la grille plutôt que de renvoyer
        // du noir. Un sommet dont l'altitude dépasse `CHUNK_HEIGHT` (réglage trop
        // ambitieux) prend ainsi la couleur du voxel le plus haut au lieu de créer
        // un trou noir. Avec une amplitude correctement bornée ce cas ne se
        // produit pas, mais le garde-fou évite une régression silencieuse.
        let mut wz = wz.clamp(0, CHUNK_HEIGHT as i32 - 1);
        let size = CHUNK_SIZE as i32;
        let coord = IVec2::new(wx.div_euclid(size), wy.div_euclid(size));
        match self.chunks.get(&coord) {
            Some(chunk) => {
                let lx = wx.rem_euclid(size) as usize;
                let ly = wy.rem_euclid(size) as usize;
                // `wz` vient de `column_height` arrondie en **f32**, alors que la
                // génération décide « plein/air » en **f64**. Quand la surface est
                // à un cheveu sous un entier, l'arrondi f32 fait viser le voxel
                // d'AIR juste au-dessus du sol → `weights` nuls → couleur noire
                // (les « étoiles noires »). On redescend au premier voxel plein
                // pour lire le vrai matériau de surface.
                while wz > 0 && chunk.get_voxel(lx, ly, wz as usize).is_air() {
                    wz -= 1;
                }
                let voxel = chunk.get_voxel(lx, ly, wz as usize);
                let mut color = Vec3::ZERO;
                for i in 0..4 {
                    let weight = voxel.weights[i] as f32 / 255.0;
                    color += material_color(voxel.materials[i]) * weight;
                }
                color
            }
            None => Vec3::ZERO,
        }
    }

    /// Borne supérieure (exclusive) de `z` à mailler : au-dessus, tout est air
    /// (aucune surface). Évite de scanner les 256 niveaux pour un terrain qui
    /// culmine bien plus bas. À revoir quand on ajoutera des grottes.
    pub fn surface_z_max(&self) -> usize {
        ((self.params.height.base_height + self.params.height.amplitude).ceil() as usize + 2)
            .min(CHUNK_HEIGHT - 1)
    }

    /// Hauteur **continue** (flottante) de la surface à la colonne monde
    /// `(wx, wy)` — exactement la valeur `surface_f` qui a rempli le champ de
    /// densité. Toujours définie (jamais `None`) et fonction des seules
    /// coordonnées monde, donc identique des deux côtés d'une couture de chunk
    /// (maillage sans fissure). C'est la source de vérité du mailleur heightfield.
    ///
    /// Lue dans le **cache de colonne** du chunk concerné (rempli à la
    /// génération) — y compris pour un chunk voisin, ce qui couvre l'apron des
    /// normales débordant d'un bord. Repli sur un calcul direct du fBm pour les
    /// rares colonnes d'un chunk non encore généré (frange du monde) : la valeur
    /// est alors *identique* à celle qu'aurait mise le cache, donc sans couture.
    pub fn column_height(&self, wx: i32, wy: i32) -> f32 {
        let size = CHUNK_SIZE as i32;
        let coord = IVec2::new(wx.div_euclid(size), wy.div_euclid(size));
        match self.chunks.get(&coord) {
            Some(chunk) => {
                let lx = wx.rem_euclid(size) as usize;
                let ly = wy.rem_euclid(size) as usize;
                chunk.column_height(lx, ly)
            }
            None => self.height.height(wx as f64, wy as f64) as f32,
        }
    }

    /// Hauteur **continue** de la surface (iso `density == ISO_LEVEL`) à la
    /// colonne monde `(wx, wy)`, par interpolation linéaire entre les deux voxels
    /// qui encadrent la traversée plein → air. Donne une hauteur fractionnaire
    /// (sous-voxel) → surface lisse, sans marches.
    ///
    /// Lue en coordonnées **monde** : deux chunks voisins calculent une valeur
    /// rigoureusement identique sur leur colonne partagée → maillage sans
    /// fissure. Renvoie `None` si la colonne n'a pas de surface (entièrement
    /// pleine ou vide) — cas qui n'arrive pas avec la génération actuelle, mais
    /// qu'on gère proprement.
    pub fn surface_height_at(&self, wx: i32, wy: i32) -> Option<f32> {
        let zmax = self.surface_z_max() as i32;
        let mut prev = self.density_at(wx, wy, 0);
        for z in 1..=zmax {
            let cur = self.density_at(wx, wy, z);
            // Traversée plein → air entre z-1 et z : `density` y vaut ISO_LEVEL.
            if prev >= ISO_LEVEL && cur < ISO_LEVEL {
                let t = (ISO_LEVEL - prev) / (cur - prev);
                return Some((z - 1) as f32 + t);
            }
            prev = cur;
        }
        None
    }
}
