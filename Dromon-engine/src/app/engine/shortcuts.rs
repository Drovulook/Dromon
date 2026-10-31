//! **Raccourcis clavier** du moteur (hors déplacement de la caméra, cf. `Camera::update`),
//! lus à chaque frame.

use crate::app::engine::Engine;
use winit::keyboard::KeyCode;
use winit::window::WindowId;

impl Engine {
    pub(super) fn handle_shortcuts(&mut self, window_id: WindowId) {
        let input = self.input_manager.input_state();
        if input.just_pressed(KeyCode::F12) {
            self.manual_screenshot(window_id);
        }
    }
}
