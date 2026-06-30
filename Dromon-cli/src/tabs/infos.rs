use crate::app_state::AppState;
use crate::{BG, BORDER, FG, TITLE};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Span,
    widgets::{Block, Borders, List, ListItem},
};

/// Onglet « logs » : liste défilante des messages reçus de l'engine.
pub fn render(frame: &mut Frame, state: &mut AppState, area: Rect) {
    let items: Vec<ListItem> = state
        .logs
        .iter()
        .map(|l| ListItem::new(l.clone()))
        .collect();

    let title = if state.auto_scroll {
        "Logs — (↑↓ scroll  g/G haut/bas  q quitter)"
    } else {
        "Logs — [scroll manuel] (G = bas + auto)"
    };

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(TITLE)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG).fg(FG));

    frame.render_stateful_widget(List::new(items).block(block), area, &mut state.list_state);
}
