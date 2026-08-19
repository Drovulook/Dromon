//! **Frustum culling des chunks**, recalculé à chaque frame.
//!
//! Deux listes et non une : un chunk derrière la caméra ne se dessine pas, mais peut très
//! bien projeter une ombre dans le champ de vision. Les deux frustums sont donc testés
//! indépendamment, en un seul balayage des coordonnées.
//!
//! Le test est **conservatif** (cf. [`Frustum::intersects_aabb`]) : jamais de faux
//! négatif, donc jamais de géométrie qui disparaît — au pire un chunk dessiné pour rien.

use glam::IVec2;

use crate::app::engine::renderer::camera::Camera;
use crate::app::engine::renderer::frustum_culling::Frustum;
use crate::app::engine::renderer::light::DirectionalLight;
use crate::app::engine::renderer::world::terrain::meshes::LoadedChunks;
use crate::profile;

/// Chunks retenus pour la frame courante. Les `Vec` sont réutilisés d'une frame à
/// l'autre (`clear` garde la capacité) : le culling n'alloue qu'aux premières frames.
#[derive(Default)]
pub struct VisibleSet {
    /// Chunks à dessiner dans la passe principale.
    pub camera: Vec<IVec2>,
    /// Chunks à dessiner dans la passe d'ombre.
    pub shadow: Vec<IVec2>,
}

impl VisibleSet {
    pub(super) fn update(
        &mut self,
        chunks: &LoadedChunks,
        camera: &Camera,
        light: &DirectionalLight,
    ) {
        profile!();
        let camera_frustum = Frustum::from_view_proj(camera.proj * camera.view);
        let light_frustum =
            Frustum::from_view_proj(light.view_proj(camera.position, camera.front()));

        self.camera.clear();
        self.shadow.clear();
        for &coord in chunks.coords() {
            let (min, max) = Frustum::chunk_aabb(coord);
            if camera_frustum.intersects_aabb(min, max) {
                self.camera.push(coord);
            }
            if light_frustum.intersects_aabb(min, max) {
                self.shadow.push(coord);
            }
        }
    }
}
