use glam::{IVec2, Vec3};

use crate::app::engine::terrain_generation::{CHUNK_HEIGHT, CHUNK_SIZE};

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
            r2,      // near — Vulkan clippe z ∈ [0, w], pas [−w, w]
            r3 - r2, // far
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

    pub fn chunk_aabb(coord: IVec2) -> (Vec3, Vec3) {
        let size = CHUNK_SIZE as f32;
        let height = CHUNK_HEIGHT as f32;
        let min = Vec3::new(coord.x as f32 * size, coord.y as f32 * size, 0.0);
        (min, min + Vec3::new(size, size, 256.0))
    }
}
