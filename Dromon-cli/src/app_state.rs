use ratatui::widgets::ListState;
use std::sync::{Arc, atomic::AtomicBool, mpsc};

pub struct AppState {
    pub logs: Vec<String>,
    pub rx: mpsc::Receiver<String>,
    pub shutdown: Arc<AtomicBool>,
    pub list_state: ListState,
    pub auto_scroll: bool,
    pub viewport_height: usize,
}

impl AppState {
    pub fn new(rx: mpsc::Receiver<String>, shutdown: Arc<AtomicBool>) -> Self {
        Self {
            logs: Vec::new(),
            rx,
            shutdown,
            list_state: ListState::default(),
            auto_scroll: true,
            viewport_height: 1,
        }
    }

    pub fn bottom_offset(&self) -> usize {
        self.logs.len().saturating_sub(self.viewport_height)
    }

    pub fn push(&mut self, msg: String) {
        self.logs.push(msg);
        if self.auto_scroll {
            *self.list_state.offset_mut() = self.bottom_offset();
        }
    }

    pub fn scroll_up(&mut self) {
        self.auto_scroll = false;
        *self.list_state.offset_mut() = self.list_state.offset().saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        let max = self.bottom_offset();
        let new = (self.list_state.offset() + 1).min(max);
        *self.list_state.offset_mut() = new;
        if new >= max {
            self.auto_scroll = true;
        }
    }

    pub fn go_to_top(&mut self) {
        self.auto_scroll = false;
        *self.list_state.offset_mut() = 0;
    }

    pub fn go_to_bottom(&mut self) {
        self.auto_scroll = true;
        *self.list_state.offset_mut() = self.bottom_offset();
    }
}
