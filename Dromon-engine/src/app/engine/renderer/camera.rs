use glam::{Mat4, Vec3};
use winit::keyboard::KeyCode;

use crate::app::engine::events::input::InputState;
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

    /// Rayon du disque du monde (centré sur l'origine), `None` sans terrain. Seule la
    /// position horizontale est bornée : la hauteur reste libre.
    world_radius: Option<f32>,
    /// `debug.camera_unbounded` : aucune limite, pour regarder le monde de l'extérieur.
    unbounded: bool,
}

impl Camera {
    /// Caméra à sa pose de départ (`render.ron`), sans limite tant qu'il n'y a pas de terrain.
    pub fn new(params: &CameraParams, unbounded: bool) -> Camera {
        Camera {
            params: params.clone(),
            position: params.start_position,
            yaw: params.start_yaw_deg.to_radians(),
            pitch: params.start_pitch_deg.to_radians(),
            fov_y: params.fov_deg.to_radians(),
            view: Mat4::IDENTITY,
            proj: Mat4::IDENTITY,
            is_primary: true,
            world_radius: None,
            unbounded,
        }
    }

    /// Hot reload : nouveaux réglages. Une pose de départ modifiée téléporte la caméra
    /// (pratique pour régler un point de vue), un FOV modifié remplace le zoom courant.
    /// `Err` : pose de départ hors du monde, ignorée (le reste est appliqué).
    pub fn hot_reload(&mut self, params: &CameraParams) -> Result<(), String> {
        let old = &self.params;
        let mut result = Ok(());
        if params.start_position != old.start_position
            || params.start_yaw_deg != old.start_yaw_deg
            || params.start_pitch_deg != old.start_pitch_deg
        {
            result = self.check_inside(params.start_position);
            if result.is_ok() {
                self.position = params.start_position;
                self.yaw = params.start_yaw_deg.to_radians();
                self.pitch = params.start_pitch_deg.to_radians();
            }
        }
        if params.fov_deg != old.fov_deg {
            self.fov_y = params.fov_deg.to_radians();
        }
        self.params = params.clone();
        result
    }

    /// Nouvelles limites (terrain créé ou régénéré, `camera_unbounded` modifié). `true`
    /// si la caméra était dehors et a été ramenée au bord.
    pub fn set_bounds(&mut self, world_radius: Option<f32>, unbounded: bool) -> bool {
        self.world_radius = world_radius;
        self.unbounded = unbounded;
        let clamped = self.clamp(self.position);
        let moved = clamped != self.position;
        self.position = clamped;
        moved
    }

    /// Rayon horizontal autorisé, `None` → déplacement libre.
    fn limit(&self) -> Option<f32> {
        self.world_radius.filter(|_| !self.unbounded)
    }

    /// Ramène `position` sur le bord du disque si elle en sort, hauteur gardée. Projeter
    /// plutôt que refuser le mouvement : la caméra glisse le long du bord au lieu d'y coller.
    fn clamp(&self, position: Vec3) -> Vec3 {
        let Some(radius) = self.limit() else {
            return position;
        };
        let horizontal = position.truncate();
        let distance = horizontal.length();
        if distance <= radius {
            return position;
        }
        (horizontal * (radius / distance)).extend(position.z)
    }

    fn check_inside(&self, position: Vec3) -> Result<(), String> {
        match self.limit() {
            Some(radius) if position.truncate().length() > radius => Err(format!(
                "({:.0}, {:.0}) hors du monde : distance horizontale au centre {:.0} > rayon {radius:.0}",
                position.x,
                position.y,
                position.truncate().length(),
            )),
            _ => Ok(()),
        }
    }

    /// Déplacement instantané (commande). Angles en radians, `None` → inchangés ; pitch
    /// borné comme à la souris. Matrices recalculées à la frame suivante. Position hors
    /// du monde → refusée, caméra inchangée.
    pub fn teleport(
        &mut self,
        position: Vec3,
        yaw: Option<f32>,
        pitch: Option<f32>,
    ) -> Result<(), String> {
        self.check_inside(position)?;
        self.position = position;
        if let Some(yaw) = yaw {
            self.yaw = yaw;
        }
        if let Some(pitch) = pitch {
            self.pitch = pitch.clamp(-MAX_PITCH, MAX_PITCH);
        }
        Ok(())
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
    #[allow(dead_code)] // plus utilisée depuis la pose de départ en config, gardée pour plus tard
    pub fn look_at(&mut self, target: Vec3) {
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
            self.position = self.clamp(self.position + direction.normalize() * p.move_speed * boost * dt);
        }

        self.recompute_matrices(aspect);
    }

    /// Reconstruit `view` et `proj` depuis l'état courant.
    fn recompute_matrices(&mut self, aspect: f32) {
        let front = self.front();
        self.view = Mat4::look_at_rh(self.position, self.position + front, Vec3::Z);

        // Reversed-Z : near et far échangés → profondeur 1 au plan near, 0 au plan far.
        // Avec un depth buffer flottant, la précision devient ~relative à la distance
        // (sinon ∝ d²/near : z-fighting eau/plage au loin). Tests en GREATER, clear à 0.
        let mut proj = Mat4::perspective_rh(self.fov_y, aspect, self.params.far, self.params.near);
        proj.y_axis.y *= -1.0; // Vulkan a l'axe Y de l'écran vers le bas
        self.proj = proj;
    }
}
