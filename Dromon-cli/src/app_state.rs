use crate::log_parser::{self, ParsedMessage};
use ratatui::{text::Line, widgets::ListState};
use std::sync::{Arc, atomic::AtomicBool, mpsc};

pub struct AppState {
    pub logs: Vec<Line<'static>>,
    pub rx: mpsc::Receiver<String>,
    pub shutdown: Arc<AtomicBool>,
    pub list_state: ListState,
    pub auto_scroll: bool,
    pub viewport_height: usize,
    pub fps: Option<f32>,
    pub state: Option<String>,
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
            fps: None,
            state: None,
        }
    }

    pub fn bottom_offset(&self) -> usize {
        self.logs.len().saturating_sub(self.viewport_height)
    }

    pub fn push(&mut self, msg: String) {
        match log_parser::parse(msg) {
            ParsedMessage::Log(line) => {
                self.logs.push(line);
                if self.auto_scroll {
                    *self.list_state.offset_mut() = self.bottom_offset();
                }
            }
            ParsedMessage::Fps(fps) => self.fps = Some(fps),
            ParsedMessage::State(s) => self.state = Some(s),
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
