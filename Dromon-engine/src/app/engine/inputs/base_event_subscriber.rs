use crate::app::{engine::inputs::InputManager, logger::Logger};
use std::sync::Arc;

pub fn base_event_subscriber(input_manager: &mut InputManager, logger_for_callback: Arc<Logger>) {
    input_manager.subscribe(move |event| {
        // logger_for_callback.info(&format!("input event: {event:?}"));
    });
}
