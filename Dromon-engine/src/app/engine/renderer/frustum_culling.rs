use glam::{IVec2, Vec3};

use crate::app::engine::terrain_generation::CHUNK_SIZE;

// pistes d'amélioration:
// - éviter d'être trop conservateur sur la hauteur des chunks
// - implémenter culling pour modèles 3D

pub struct Frustum {
    planes: [glam::Vec4; 6],
}

impl Frustum {
    pub fn from_view_proj(m: glam::Mat4) -> Self {
        // ⚠ glam est column-major : m.x_axis est la COLONNE 0, pas la ligne 0.
        // Mat4::row(i) donne bien la ligne — c'est elle qu'il faut ici.
        let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));

        let mut planes = [
            r3 + r0, // left
            r3 - r0, // right
            r3 + r1, // bottom
            r3 - r1, // top
            // Vulkan clippe z ∈ [0, w], pas [−w, w]. z = 0 est le plan near en
            // projection classique (lumière), le plan far en reversed-Z (caméra) :
            // les deux plans bornent le volume dans les deux cas.
            r2,
            r3 - r2,
        ];

        // Normaliser : la distance signée devient une vraie distance en unités
        // monde. Inutile pour un test binaire, mais permet d'ajouter une marge.
        for p in &mut planes {
            *p /= p.truncate().length();
        }
        Self { planes }
    }

    /// `false` ⇒ la boîte est certainement invisible. `true` ⇒ elle l'est
    /// probablement (test conservatif : quelques faux positifs près des coins
    /// du frustum, jamais de faux négatif; on ne fait donc jamais disparaître
    /// de géométrie, on dessine au pire un chunk de trop).
    pub fn intersects_aabb(&self, min: glam::Vec3, max: glam::Vec3) -> bool {
        for plane in &self.planes {
            let n = plane.truncate();
            // p-vertex : le coin le plus « en avant » selon n
            let p = glam::Vec3::new(
                if n.x >= 0.0 { max.x } else { min.x },
                if n.y >= 0.0 { max.y } else { min.y },
                if n.z >= 0.0 { max.z } else { min.z },
            );
            if n.dot(p) + plane.w < 0.0 {
                return false; // entièrement derrière ce plan
            }
        }
        true
    }
}

/// [`Frustum::intersects_aabb`] spécialisé pour les colonnes de chunks (`CHUNK_SIZE` de
/// côté, du sol au plafond du monde `max_height`) : toutes ont la même forme, donc pour un plan donné le coin le plus « en avant » est le
/// même pour tous les chunks, à leur décalage près. Le choix du coin se fait une fois par
/// plan ; par chunk, il reste `a·cx + b·cy + c ≥ 0`. Même résultat, aux arrondis près.
pub struct ChunkCuller {
    /// `(a, b, c)` par plan.
    planes: [[f32; 3]; 6],
}

impl ChunkCuller {
    pub fn new(frustum: &Frustum, max_height: f32) -> Self {
        let size = CHUNK_SIZE as f32;
        let planes = frustum.planes.map(|plane| {
            // Coin le plus en avant, relatif à l'origine (cx·size, cy·size, 0) du chunk.
            let corner = Vec3::new(
                if plane.x >= 0.0 { size } else { 0.0 },
                if plane.y >= 0.0 { size } else { 0.0 },
                if plane.z >= 0.0 { max_height } else { 0.0 },
            );
            // n·(origine + coin) + w = (n.x·size)·cx + (n.y·size)·cy + (n·coin + w)
            [
                plane.x * size,
                plane.y * size,
                plane.truncate().dot(corner) + plane.w,
            ]
        });
        Self { planes }
    }

    /// `false` ⇒ le chunk est certainement invisible (cf. [`Frustum::intersects_aabb`]).
    #[inline]
    pub fn contains(&self, coord: IVec2) -> bool {
        let (x, y) = (coord.x as f32, coord.y as f32);
        // `&` et non `&&` : les 6 plans sans branchement, moins cher qu'une sortie anticipée.
        self.planes
            .iter()
            .fold(true, |inside, &[a, b, c]| inside & (a * x + b * y + c >= 0.0))
    }
}
