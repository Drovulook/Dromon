use glam::{Vec2, Vec3};
use std::sync::Arc;

use crate::app::engine::renderer::frustum_culling::Frustum;
use crate::app::engine::terrain_generation::TerrainSource;
use crate::config::WaterRenderParams;

use super::{LAND_MARGIN, WaterNode};

/// **Quadtree CDLOD de la mer**, figé à la création : seule la sélection des nœuds
/// (`select`) change d'une frame à l'autre.
///
/// Le niveau 0 est le plus fin (`leaf_size`) ; chaque niveau double la taille des
/// nœuds et leur portée.
pub(super) struct Quadtree {
    /// Coin (x min, y min) de la racine, qui couvre tout le monde.
    root_origin: Vec2,
    root_level: u32,
    leaf_size: f32,
    range_factor: f32,
    morph_start_ratio: f32,
    /// Rayon du disque du monde : les nœuds entièrement hors du disque sont ignorés.
    world_radius: f32,
    /// Distance horizontale d'affichage (portée de chargement du terrain).
    max_distance: f32,
    /// Pour la distance à la côte : les nœuds entièrement sur terre sont ignorés.
    source: Arc<TerrainSource>,
}

/// Ce dont la sélection a besoin de la caméra, pour une frame.
pub(super) struct Viewer<'a> {
    pub position: Vec3,
    pub far: f32,
    pub frustum: &'a Frustum,
}

/// La surface à découper : plan `sea_level`, dont les vagues s'écartent d'au plus
/// `height_margin`, verticalement comme horizontalement (frustum culling seulement).
pub(super) struct Surface {
    pub sea_level: f32,
    pub height_margin: f32,
}

impl Quadtree {
    pub fn new(
        params: &WaterRenderParams,
        world_radius: f32,
        max_distance: f32,
        source: Arc<TerrainSource>,
    ) -> Quadtree {
        // Plus petit L tel que leaf · 2^L couvre le diamètre du monde.
        let root_level = (2.0 * world_radius / params.leaf_size)
            .log2()
            .ceil()
            .max(0.0) as u32;
        let root_size = params.leaf_size * (1u64 << root_level) as f32;
        Quadtree {
            root_origin: Vec2::splat(-root_size / 2.0),
            root_level,
            leaf_size: params.leaf_size,
            range_factor: params.range_factor,
            morph_start_ratio: params.morph_start_ratio,
            world_radius,
            max_distance,
            source,
        }
    }

    /// Côté d'un nœud de niveau `level`.
    fn node_size(&self, level: u32) -> f32 {
        self.leaf_size * (1u64 << level) as f32
    }

    /// Remplit `out` avec les nœuds à dessiner cette frame.
    pub fn select(&self, viewer: &Viewer, surface: &Surface, out: &mut Vec<WaterNode>) {
        out.clear();
        self.visit(self.root_origin, self.root_level, viewer, surface, out);
    }

    fn visit(
        &self,
        origin: Vec2,
        level: u32,
        viewer: &Viewer,
        surface: &Surface,
        out: &mut Vec<WaterNode>,
    ) {
        let size = self.node_size(level);

        // Rejets : hors du monde, trop loin, sur terre, hors du champ de vision. Un
        // nœud rejeté l'est avec tous ses enfants (ils sont contenus dedans).
        if distance_to_square(Vec2::ZERO, origin, size) > self.world_radius {
            return;
        }
        // Horizontale, comme le chargement du terrain et le `discard` du shader.
        if distance_to_square(viewer.position.truncate(), origin, size) > self.max_distance {
            return;
        }
        // Entièrement sur terre : la côte est plus loin du centre que n'importe quel
        // point du nœud (demi-diagonale).
        let center = origin + size / 2.0;
        let half_diagonal = size * std::f32::consts::FRAC_1_SQRT_2;
        if self.source.coast_distance(center.x, center.y) - half_diagonal > LAND_MARGIN {
            return;
        }
        // Distance au carré PLAT, comme `k` dans le shader (calculé avant les vagues) :
        // la preuve d'absence de fissure repose sur cette cohérence.
        let distance = distance_to_box(
            viewer.position,
            origin.extend(surface.sea_level),
            (origin + size).extend(surface.sea_level),
        );
        // Gerstner déplace aussi les sommets à l'horizontale, d'au plus la même marge.
        let margin = surface.height_margin;
        let visible = viewer.frustum.intersects_aabb(
            (origin - margin).extend(surface.sea_level - margin),
            (origin + size + margin).extend(surface.sea_level + margin),
        );
        if distance > viewer.far || !visible {
            return;
        }

        // Portée du niveau ; découpé si la caméra est dans celle des enfants (la moitié).
        let range = self.range_factor * size;
        if level == 0 || distance >= range / 2.0 {
            out.push(WaterNode {
                origin,
                size,
                // Transition sur la fin de la zone [range/2, range] du niveau.
                morph_start: range * (1.0 + self.morph_start_ratio) / 2.0,
                morph_end: range,
            });
            return;
        }
        let half = size / 2.0;
        for offset in [
            Vec2::ZERO,
            Vec2::new(half, 0.0),
            Vec2::new(0.0, half),
            Vec2::splat(half),
        ] {
            self.visit(origin + offset, level - 1, viewer, surface, out);
        }
    }
}

/// Distance de `p` au carré `[origin, origin + size]²` (0 si dedans).
fn distance_to_square(p: Vec2, origin: Vec2, size: f32) -> f32 {
    let d = (origin - p).max(p - (origin + size)).max(Vec2::ZERO);
    d.length()
}

/// Distance de `p` à la boîte `[min, max]` (0 si dedans).
fn distance_to_box(p: Vec3, min: Vec3, max: Vec3) -> f32 {
    let d = (min - p).max(p - max).max(Vec3::ZERO);
    d.length()
}
