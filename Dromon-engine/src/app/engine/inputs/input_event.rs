use winit::keyboard::KeyCode;

#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    KeyPressed(KeyCode),
    KeyReleased(KeyCode),
    MouseMotion { dx: f64, dy: f64 }, // déplacement relatif (DeviceEvent)
    CursorMoved { x: f64, y: f64 },   // position absolue dans la fenêtre (WindowEvent)
    Scroll { delta: f32 },
}
