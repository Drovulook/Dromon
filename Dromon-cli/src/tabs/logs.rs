use crate::{BG, BORDER, FG, GOLD, MUTED};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

/// Barre de commande en bas de l'onglet : `:` pour saisir, Entrée envoie, Échap annule.
#[derive(Default)]
pub struct CommandInput {
    text: String,
    editing: bool,
}

impl CommandInput {
    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn start(&mut self) {
        self.editing = true;
    }

    pub fn cancel(&mut self) {
        self.text.clear();
        self.editing = false;
    }

    pub fn push(&mut self, c: char) {
        self.text.push(c);
    }

    pub fn pop(&mut self) {
        self.text.pop();
    }

    /// Texte saisi ; la barre est vidée et quitte la saisie.
    pub fn take(&mut self) -> String {
        self.editing = false;
        std::mem::take(&mut self.text)
    }
}

/// État propre à l'onglet « logs » : la liste des messages reçus de l'engine et
/// la position de défilement. `auto_scroll` colle la vue au bas tant que
/// l'utilisateur n'a pas scrollé manuellement vers le haut.
pub struct LogsState {
    logs: Vec<Line<'static>>,
    list_state: ListState,
    auto_scroll: bool,
    viewport_height: usize,
    /// Lignes d'écran après retour à la ligne (unité du défilement). Dépend de la
    /// largeur : recalculé à chaque rendu.
    rows: usize,
    pub input: CommandInput,
}

impl LogsState {
    pub fn new() -> Self {
        Self {
            logs: Vec::new(),
            list_state: ListState::default(),
            auto_scroll: true,
            viewport_height: 1,
            rows: 0,
            input: CommandInput::default(),
        }
    }

    /// Ajoute une ligne. Le suivi du bas se fait au rendu, qui connaît le nombre de
    /// lignes d'écran.
    pub fn push_line(&mut self, line: Line<'static>) {
        self.logs.push(line);
    }

    /// Offset qui place la dernière ligne d'écran en bas de la zone visible.
    fn bottom_offset(&self) -> usize {
        self.rows.saturating_sub(self.viewport_height)
    }

    pub fn scroll_up(&mut self) {
        self.auto_scroll = false;
        *self.list_state.offset_mut() = self.list_state.offset().saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        let max = self.bottom_offset();
        let new = (self.list_state.offset() + 1).min(max);
        *self.list_state.offset_mut() = new;
        // Revenu tout en bas : on réactive le suivi automatique.
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

/// Onglet « logs » : liste défilante des messages reçus de l'engine, barre de commande
/// en dessous.
pub fn render(frame: &mut Frame, state: &mut LogsState, area: Rect) {
    let [area, input_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(area);
    render_input(frame, &state.input, input_area);

    // Taille visible = zone allouée moins les bordures. Les lignes longues sont
    // découpées à cette largeur ; on recolle ensuite la vue au bas si besoin.
    let width = (area.width as usize).saturating_sub(4);
    let items: Vec<ListItem> = state
        .logs
        .iter()
        .flat_map(|l| wrap(l, width))
        .map(ListItem::new)
        .collect();
    state.rows = items.len();
    state.viewport_height = (area.height as usize).saturating_sub(2).max(1);
    if state.auto_scroll {
        *state.list_state.offset_mut() = state.bottom_offset();
    }

    let title = if state.auto_scroll {
        "(↑↓ scroll  g/G haut/bas  : commande  q quitter)"
    } else {
        "[scroll manuel] (G = bas + auto)"
    };

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(GOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG).fg(FG));

    frame.render_stateful_widget(List::new(items).block(block), area, &mut state.list_state);
}

/// Retrait des lignes de suite d'un message trop long.
const WRAP_INDENT: &str = "  ";

/// Découpe `line` en lignes d'au plus `width` colonnes, couleurs conservées. Coupure
/// au caractère (les chemins, lignes longues typiques, n'ont pas d'espace) ; 1
/// caractère = 1 colonne, faux seulement pour les caractères larges (emoji, CJK).
fn wrap(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    // Trop étroit pour progresser après le retrait : on laisse ratatui couper.
    if width <= WRAP_INDENT.len() {
        return vec![line.clone()];
    }
    let mut rows = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for span in &line.spans {
        let mut chunk = String::new();
        for c in span.content.chars() {
            if used >= width {
                if !chunk.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut chunk), span.style));
                }
                rows.push(Line::from(std::mem::take(&mut spans)).style(line.style));
                spans.push(Span::raw(WRAP_INDENT));
                used = WRAP_INDENT.len();
            }
            chunk.push(c);
            used += 1;
        }
        if !chunk.is_empty() {
            spans.push(Span::styled(chunk, span.style));
        }
    }
    rows.push(Line::from(spans).style(line.style));
    rows
}

const PROMPT: &str = "> ";

fn render_input(frame: &mut Frame, input: &CommandInput, area: Rect) {
    let (text, color, title) = if input.editing {
        (
            format!("{PROMPT}{}", input.text),
            FG,
            "commande (Entrée envoyer  Échap annuler)",
        )
    } else {
        (
            "ex. camera.get   camera.teleport pos=0,0,300   time.set hour=18".to_owned(),
            MUTED,
            "commande (:)",
        )
    };
    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(GOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if input.editing { GOLD } else { BORDER }))
        .style(Style::default().bg(BG));
    frame.render_widget(
        Paragraph::new(Span::styled(text, Style::default().fg(color))).block(block),
        area,
    );

    if input.editing {
        let typed = (PROMPT.chars().count() + input.text.chars().count()) as u16;
        frame.set_cursor_position(Position::new(area.x + 1 + typed, area.y + 1));
    }
}
