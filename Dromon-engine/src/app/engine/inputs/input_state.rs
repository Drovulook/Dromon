use std::collections::HashSet;
use winit::keyboard::KeyCode;

use crate::app::engine::inputs::InputEvent;

#[derive(Default)]
pub struct InputState {
    held: HashSet<KeyCode>,
    just_pressed: HashSet<KeyCode>,
    just_released: HashSet<KeyCode>,
    mouse_delta: (f64, f64),     // déplacement souris cumulé sur la frame
    scroll_delta: f32,           // scroll cumulé sur la frame
    cursor_position: (f64, f64), // dernière position absolue connue
}

impl InputState {
    pub fn is_held(&self, key: KeyCode) -> bool {
        self.held.contains(&key)
    }
    pub fn just_pressed(&self, key: KeyCode) -> bool {
        self.just_pressed.contains(&key)
    }
    pub fn just_released(&self, key: KeyCode) -> bool {
        self.just_released.contains(&key)
    }
    pub fn mouse_delta(&self) -> (f64, f64) {
        self.mouse_delta
    }
    pub fn scroll_delta(&self) -> f32 {
        self.scroll_delta
    }
    pub fn cursor_position(&self) -> (f64, f64) {
        self.cursor_position
    }

    // Mise à jour, appelée par le dispatcher à chaque event
    pub(super) fn apply(&mut self, event: &InputEvent) {
        match event {
            InputEvent::KeyPressed(code) => {
                self.held.insert(*code);
                self.just_pressed.insert(*code);
            }
            InputEvent::KeyReleased(code) => {
                self.held.remove(code);
                self.just_released.insert(*code);
            }
            InputEvent::MouseMotion { dx, dy } => {
                self.mouse_delta.0 += dx;
                self.mouse_delta.1 += dy;
            }
            InputEvent::Scroll { delta } => {
                self.scroll_delta += delta;
            }
            InputEvent::CursorMoved { x, y } => {
                self.cursor_position = (*x, *y);
            }
        }
    }

    /// À appeler à la FIN de chaque frame : on efface ce qui est ponctuel,
    /// on garde `held`.
    pub(super) fn end_frame(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
        self.mouse_delta = (0.0, 0.0);
        self.scroll_delta = 0.0;
    }

    // appelé quand la fenêtre perd le focus, (pour éviter que la caméra continue à se déplacer par
    // ex)
    pub(super) fn clear(&mut self) {
        self.held.clear();
        self.end_frame();
    }
}
