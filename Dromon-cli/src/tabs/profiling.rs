use crate::{BG, BORDER, FG, TITLE};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Span,
    widgets::{Block, Borders},
};

/// Onglet « profiling » : encadrés vides pour l'instant. « initialize » (timings
/// de démarrage, plus grand) en haut ; en dessous, « frame » (CPU par frame) et
/// « GPU » (décomposition du temps de rendu) côte à côte.
pub fn render(frame: &mut Frame, area: Rect) {
    let [init_area, bottom_area] =
        Layout::vertical([Constraint::Fill(3), Constraint::Fill(2)]).areas(area);
    let [frame_area, gpu_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(bottom_area);

    for (title, rect) in [
        ("initialize", init_area),
        ("frame", frame_area),
        ("GPU", gpu_area),
    ] {
        let block = Block::default()
            .title(Span::styled(title, Style::default().fg(TITLE)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(BG).fg(FG));
        frame.render_widget(block, rect);
    }
}
