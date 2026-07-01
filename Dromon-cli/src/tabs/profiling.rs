use crate::{BG, BLUE, BORDER, FG, GOLD};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders},
};

/// Les trois encadrés de l'onglet, disposés ainsi :
///   initialize        (en haut, pleine largeur)
///   frame    gpu      (en bas, côte à côte)
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProfBlock {
    Initialize,
    Frame,
    Gpu,
}

/// État propre à l'onglet « profiling » : quel encadré est sélectionné et si on
/// est en mode zoom (un seul encadré occupe toute la zone).
pub struct ProfilingState {
    selected: ProfBlock,
    zoomed: bool,
}

impl ProfilingState {
    pub fn new() -> Self {
        Self {
            selected: ProfBlock::Initialize,
            zoomed: false,
        }
    }

    // Navigation dans la grille : `initialize` en haut, `frame`/`gpu` en bas.
    // Chaque déplacement ne fait rien s'il n'y a pas de bloc dans la direction.
    pub fn move_up(&mut self) {
        if matches!(self.selected, ProfBlock::Frame | ProfBlock::Gpu) {
            self.selected = ProfBlock::Initialize;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected == ProfBlock::Initialize {
            self.selected = ProfBlock::Frame;
        }
    }

    pub fn move_left(&mut self) {
        if self.selected == ProfBlock::Gpu {
            self.selected = ProfBlock::Frame;
        }
    }

    pub fn move_right(&mut self) {
        if self.selected == ProfBlock::Frame {
            self.selected = ProfBlock::Gpu;
        }
    }

    pub fn toggle_zoom(&mut self) {
        self.zoomed = !self.zoomed;
    }
}

/// Onglet « profiling » : encadrés vides pour l'instant. « initialize » (timings
/// de démarrage, plus grand) en haut ; en dessous, « frame » (CPU par frame) et
/// « GPU » (décomposition du temps de rendu) côte à côte. L'encadré sélectionné
/// a une bordure bleue en gras ; en mode zoom il occupe toute la zone.
pub fn render(frame: &mut Frame, state: &ProfilingState, area: Rect) {
    // Dessine un encadré, mis en avant s'il est sélectionné.
    let draw = |frame: &mut Frame, title: &str, id: ProfBlock, rect: Rect| {
        let border_style = if id == state.selected {
            Style::default().fg(BLUE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(BORDER)
        };
        let block = Block::default()
            .title(Span::styled(title, Style::default().fg(GOLD)))
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(Style::default().bg(BG).fg(FG));
        frame.render_widget(block, rect);
    };

    // Mode zoom : le bloc sélectionné occupe toute la zone sous la barre d'onglets.
    if state.zoomed {
        let title = match state.selected {
            ProfBlock::Initialize => "initialize",
            ProfBlock::Frame => "frame",
            ProfBlock::Gpu => "GPU",
        };
        draw(frame, title, state.selected, area);
        return;
    }

    let [init_area, bottom_area] =
        Layout::vertical([Constraint::Fill(2), Constraint::Fill(2)]).areas(area);
    let [frame_area, gpu_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(bottom_area);

    draw(frame, "initialize", ProfBlock::Initialize, init_area);
    draw(frame, "frame", ProfBlock::Frame, frame_area);
    draw(frame, "GPU", ProfBlock::Gpu, gpu_area);
}
