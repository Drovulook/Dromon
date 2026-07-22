use crate::log_parser::{self, ParsedMessage};
use crate::tabs::logs::LogsState;
use crate::tabs::profiling::ProfilingState;
use crate::tabs::world::WorldState;
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

/// État global de l'application : les bandeaux du haut (Dromon Engine, config),
/// la barre des onglets et le canal de réception. L'état et la logique propres à
/// chaque onglet vivent dans son module (`tabs::logs`, `tabs::profiling`) ; ici
/// on ne fait que router les messages reçus vers le bon sous-état.
pub struct AppState {
    pub rx: mpsc::Receiver<String>,
    pub shutdown: Arc<AtomicBool>,

    // Bandeau « Dromon Engine ».
    pub fps: Option<f32>,
    pub state: Option<String>,

    // Bandeau « config ».
    pub config_mode: Option<String>,
    pub config_profiling: Option<bool>,

    // Barre des onglets.
    pub active_tab: Tab,

    // État propre à chaque onglet.
    pub logs: LogsState,
    pub profiling: ProfilingState,
    pub world: WorldState,
}

impl AppState {
    pub fn new(rx: mpsc::Receiver<String>, shutdown: Arc<AtomicBool>) -> Self {
        Self {
            rx,
            shutdown,
            fps: None,
            state: None,
            config_mode: None,
            config_profiling: None,
            active_tab: Tab::Logs,
            logs: LogsState::new(),
            profiling: ProfilingState::new(),
            world: WorldState::new(),
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

    /// Route un message reçu de l'engine vers le bon sous-état.
    pub fn push(&mut self, msg: String) {
        match log_parser::parse(msg) {
            ParsedMessage::Log(line) => self.logs.push_line(line),
            ParsedMessage::Fps(fps) => self.fps = Some(fps),
            ParsedMessage::State(s) => self.state = Some(s),
            ParsedMessage::Config { mode, profiling } => {
                self.config_mode = Some(mode);
                self.config_profiling = Some(profiling);
            }
            ParsedMessage::ProfileInit(nodes) => self.profiling.load_init(nodes),
            ParsedMessage::ProfileFrame(nodes) => self.profiling.load_frame(nodes),
            ParsedMessage::ProfileGpu(nodes) => self.profiling.load_gpu(nodes),
            ParsedMessage::World(stats) => self.world.load(stats),
        }
    }
}
