mod base_event_subscriber;
mod input_event;
mod input_state;

pub use crate::app::engine::inputs::input_state::InputState;
pub use base_event_subscriber::base_event_subscriber;
pub use input_event::InputEvent;
use winit::event::{DeviceEvent, ElementState, MouseScrollDelta, WindowEvent};
use winit::keyboard::PhysicalKey;

// HACK: utiliser un système de polling pour plus tard

/// Un abonné : n'importe quelle closure qui réagit à un InputEvent.
type Callback = Box<dyn FnMut(&InputEvent)>;

#[derive(Default)]
pub struct InputManager {
    subscribers: Vec<Callback>,
    input_state: InputState,
}

impl InputManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// N'importe quelle partie du code s'abonne ici.
    pub fn subscribe(&mut self, callback: impl FnMut(&InputEvent) + 'static) {
        self.subscribers.push(Box::new(callback));
    }

    /// Diffuse un event moteur à tous les abonnés.
    fn dispatch(&mut self, event: InputEvent) {
        self.input_state.apply(&event);
        for cb in &mut self.subscribers {
            cb(&event);
        }
    }

    pub fn input_state(&self) -> &InputState {
        &self.input_state
    }

    pub fn end_frame(&mut self) {
        self.input_state.end_frame();
    }

    /// Traduit les WindowEvent winit → InputEvent (clavier, scroll).
    pub fn handle_window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                // ignore les répétitions auto du clavier
                if event.repeat {
                    return;
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    let input = match event.state {
                        ElementState::Pressed => InputEvent::KeyPressed(code),
                        ElementState::Released => InputEvent::KeyReleased(code),
                    };
                    self.dispatch(input);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.dispatch(InputEvent::CursorMoved {
                    x: position.x,
                    y: position.y,
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                self.dispatch(InputEvent::Scroll { delta: scroll });
            }
            // Perte de focus : on purge l'état, sinon une touche tenue au
            // moment du changement de focus reste « bloquée » (release perdu).
            WindowEvent::Focused(false) => {
                self.input_state.clear();
            }
            _ => {}
        }
    }

    /// Le déplacement *brut* de souris arrive via DeviceEvent, pas WindowEvent.
    pub fn handle_device_event(&mut self, event: &DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            self.dispatch(InputEvent::MouseMotion { dx: *dx, dy: *dy });
        }
    }
}
