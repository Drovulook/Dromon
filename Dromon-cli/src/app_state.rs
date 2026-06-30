use crate::log_parser::{self, ParsedMessage};
use ratatui::{text::Line, widgets::ListState};
use std::sync::{Arc, atomic::AtomicBool, mpsc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Logs,
    Profiling,
    World,
}

impl Tab {
    /// Ordre d'affichage dans la barre d'onglets ; sert aussi à l'index de sélection.
    pub const ALL: [Tab; 3] = [Tab::Logs, Tab::Profiling, Tab::World];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Logs => "logs",
            Tab::Profiling => "profiling",
            Tab::World => "world",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

pub struct AppState {
    pub logs: Vec<Line<'static>>,
    pub rx: mpsc::Receiver<String>,
    pub shutdown: Arc<AtomicBool>,
    pub list_state: ListState,
    pub auto_scroll: bool,
    pub viewport_height: usize,
    pub fps: Option<f32>,
    pub state: Option<String>,
    pub config_mode: Option<String>,
    pub config_profiling: Option<bool>,
    pub active_tab: Tab,
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
            config_mode: None,
            config_profiling: None,
            active_tab: Tab::Logs,
        }
    }

    pub fn next_tab(&mut self) {
        let next = (self.active_tab.index() + 1) % Tab::ALL.len();
        self.active_tab = Tab::ALL[next];
    }

    pub fn prev_tab(&mut self) {
        let i = self.active_tab.index();
        let prev = (i + Tab::ALL.len() - 1) % Tab::ALL.len();
        self.active_tab = Tab::ALL[prev];
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
            ParsedMessage::Config { mode, profiling } => {
                self.config_mode = Some(mode);
                self.config_profiling = Some(profiling);
            }
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
