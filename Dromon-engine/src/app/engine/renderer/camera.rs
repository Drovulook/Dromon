use glam::{Mat4, Vec3};
use winit::keyboard::KeyCode;

use crate::app::engine::inputs::InputState;
use crate::app::engine::timer::Timer;
use crate::config::CameraParams;
use crate::profile;

const MAX_PITCH: f32 = 1.55; // ≈ 89° : on ne regarde jamais pile à la verticale

/// Caméra du monde. Monde en Z-up (le « haut » est l'axe +Z).
pub struct Camera {
    /// Réglages (`render.ron`) : projection et contrôles.
    pub params: CameraParams,

    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// FOV vertical courant (radians), modifié par le zoom.
    pub fov_y: f32,

    pub view: Mat4,
    pub proj: Mat4,

    pub is_primary: bool,
}

impl Camera {
    pub fn new(params: &CameraParams) -> Camera {
        let mut camera = Camera {
            params: params.clone(),
            // Remontée au-dessus du plafond du monde par `generate_terrain`.
            position: Vec3::new(0.0, 0.0, 1100.0),
            yaw: 0.0,
            pitch: 0.0,
            fov_y: params.fov_deg.to_radians(),
            view: Mat4::IDENTITY,
            proj: Mat4::IDENTITY,
            is_primary: true,
        };
        // au démarrage, on regarde l'horizon vers +X : cible à la même altitude → pitch 0
        camera.look_at(camera.position + Vec3::X - Vec3::Z);
        camera
    }

    /// Vecteur « avant » (direction du regard), reconstruit depuis yaw/pitch.
    pub fn front(&self) -> Vec3 {
        Vec3::new(
            self.pitch.cos() * self.yaw.cos(),
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
        )
        .normalize()
    }

    /// Oriente la caméra vers un point (déduit yaw/pitch de la direction).
    fn look_at(&mut self, target: Vec3) {
        let dir = (target - self.position).normalize();
        self.yaw = dir.y.atan2(dir.x);
        self.pitch = dir.z.asin();
    }

    /// Lit l'état clavier et déplace la caméra, puis recalcule les matrices.
    pub fn update(&mut self, input: &InputState, timer: &Timer, aspect: f32) {
        profile!();
        let dt = timer.delta_secs();
        let p = &self.params;

        // Alt maintenue → on amplifie déplacement, rotation et zoom.
        let boost = if input.is_held(KeyCode::AltLeft) {
            p.boost_factor_rot
        } else {
            1.0
        };

        // Tant que R est maintenue, la souris ne fait plus pivoter la caméra.
        if !input.is_held(KeyCode::KeyR) {
            let (dx, dy) = input.mouse_delta();
            self.yaw -= dx as f32 * p.rotation_sensitivity * boost;
            self.pitch -= dy as f32 * p.rotation_sensitivity * boost;
            self.pitch = self.pitch.clamp(-MAX_PITCH, MAX_PITCH);
        }

        self.fov_y = (self.fov_y - input.scroll_delta() * p.zoom_speed * boost)
            .clamp(p.min_fov_deg.to_radians(), p.max_fov_deg.to_radians());

        let front = self.front();
        let world_up = Vec3::Z;
        let right = front.cross(world_up).normalize();

        let mut direction = Vec3::ZERO;
        if input.is_held(KeyCode::KeyW) {
            direction += front; // Z : avant
        }
        if input.is_held(KeyCode::KeyS) {
            direction -= front; // S : arrière
        }
        if input.is_held(KeyCode::KeyA) {
            direction -= right; // Q : gauche
        }
        if input.is_held(KeyCode::KeyD) {
            direction += right; // D : droite
        }
        if input.is_held(KeyCode::KeyQ) {
            direction += world_up; // A : haut
        }
        if input.is_held(KeyCode::KeyE) {
            direction -= world_up; // E : bas
        }

        // normalize() évite d'aller plus vite en diagonale (front + right).
        let boost = if input.is_held(KeyCode::AltLeft) {
            p.boost_factor_move
        } else {
            1.0
        };
        if direction != Vec3::ZERO {
            self.position += direction.normalize() * p.move_speed * boost * dt;
        }

        self.recompute_matrices(aspect);
    }

    /// Reconstruit `view` et `proj` depuis l'état courant.
    fn recompute_matrices(&mut self, aspect: f32) {
        let front = self.front();
        self.view = Mat4::look_at_rh(self.position, self.position + front, Vec3::Z);

        let mut proj =
            Mat4::perspective_rh(self.fov_y, aspect, self.params.near, self.params.far);
        proj.y_axis.y *= -1.0; // Vulkan a l'axe Y de l'écran vers le bas
        self.proj = proj;
    }
}
