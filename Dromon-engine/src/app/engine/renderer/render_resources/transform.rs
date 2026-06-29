use glam::{EulerRot, Mat4, Quat, Vec3};

pub struct Transform {
    pub translation: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    /// Construit la matrice de transformation T·R·S à partir des composantes.
    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            self.scale,
            Quat::from_euler(
                EulerRot::XYZ,
                self.rotation.x,
                self.rotation.y,
                self.rotation.z,
            ),
            self.translation,
        )
    }
}
