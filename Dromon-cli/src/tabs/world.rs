use crate::{BG, FG};
use ratatui::{Frame, layout::Rect, style::Style, widgets::Block};

/// Onglet « world » : vide pour l'instant, ne fait que peindre le fond pour
/// rester cohérent avec le thème.
pub fn render(frame: &mut Frame, area: Rect) {
    let block = Block::default().style(Style::default().bg(BG).fg(FG));
    frame.render_widget(block, area);
}
