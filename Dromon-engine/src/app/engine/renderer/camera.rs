use glam::{Mat4, Vec3};
use winit::keyboard::KeyCode;

use crate::app::engine::inputs::InputState;
use crate::app::engine::timer::Timer;

const MAX_PITCH: f32 = 1.55; // ≈ 89° : on ne regarde jamais pile à la verticale
const MIN_FOV: f32 = 0.350; // ≈ 20°
const MAX_FOV: f32 = 1.92; // ≈ 110°

/// Caméra du monde. Monde en Z-up (le « haut » est l'axe +Z).
pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,

    pub fov_y: f32,
    pub near: f32,
    pub far: f32,

    pub view: Mat4,
    pub proj: Mat4,

    pub is_primary: bool,
    pub move_speed: f32,
    pub rotation_sensitivity: f32,
    pub zoom_speed: f32,
    pub boost_factor_rot: f32, // multiplicateur appliqué tant qu'Alt est maintenue
    pub boost_factor_move: f32,
}

impl Default for Camera {
    fn default() -> Self {
        let mut camera = Self {
            position: Vec3::new(2.0, 2.0, 260.0),
            yaw: 0.0,
            pitch: 0.0,
            fov_y: 45.0_f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            view: Mat4::IDENTITY,
            proj: Mat4::IDENTITY,
            is_primary: true,
            move_speed: 30.0,
            rotation_sensitivity: 0.002,
            zoom_speed: 0.05,
            boost_factor_rot: 4.0,
            boost_factor_move: 4.0,
        };
        // au démarrage, on regarde l'origine (déduit yaw/pitch de la direction)
        camera.look_at(Vec3::ZERO);
        camera
    }
}

impl Camera {
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
        let dt = timer.delta_secs();

        // Alt maintenue → on amplifie déplacement, rotation et zoom.
        let boost = if input.is_held(KeyCode::AltLeft) {
            self.boost_factor_rot
        } else {
            1.0
        };

        // Tant que R est maintenue, la souris ne fait plus pivoter la caméra.
        if !input.is_held(KeyCode::KeyR) {
            let (dx, dy) = input.mouse_delta();
            self.yaw -= dx as f32 * self.rotation_sensitivity * boost;
            self.pitch -= dy as f32 * self.rotation_sensitivity * boost;
            self.pitch = self.pitch.clamp(-MAX_PITCH, MAX_PITCH);
        }

        self.fov_y =
            (self.fov_y - input.scroll_delta() * self.zoom_speed * boost).clamp(MIN_FOV, MAX_FOV);

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
            self.boost_factor_move
        } else {
            1.0
        };
        if direction != Vec3::ZERO {
            self.position += direction.normalize() * self.move_speed * boost * dt;
        }

        self.recompute_matrices(aspect);
    }

    /// Reconstruit `view` et `proj` depuis l'état courant.
    fn recompute_matrices(&mut self, aspect: f32) {
        let front = self.front();
        self.view = Mat4::look_at_rh(self.position, self.position + front, Vec3::Z);

        let mut proj = Mat4::perspective_rh(self.fov_y, aspect, self.near, self.far);
        proj.y_axis.y *= -1.0; // Vulkan a l'axe Y de l'écran vers le bas
        self.proj = proj;
    }
}
