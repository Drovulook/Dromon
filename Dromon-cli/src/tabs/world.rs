//! Onglet « world » : état du terrain généré. Deux encadrés — le résumé
//! (totaux) en haut, le détail par niveau de LOD en dessous. Les données
//! arrivent en un seul message `[WORLD]`, émis à la génération du terrain.

use crate::log_parser::{WorldLod, WorldStats};
use crate::{BG, BLUE, BORDER, FG, GOLD, MUTED};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

/// État de l'onglet : le dernier lot de statistiques reçu (vide au démarrage,
/// tant que la scène n'a pas généré son terrain).
pub struct WorldState {
    summary: Option<String>,
    lods: Vec<WorldLod>,
}

impl WorldState {
    pub fn new() -> Self {
        Self {
            summary: None,
            lods: Vec::new(),
        }
    }

    /// Remplace les statistiques par le lot reçu (un terrain régénéré écrase
    /// simplement l'ancien).
    pub fn load(&mut self, stats: WorldStats) {
        self.summary = stats.summary;
        self.lods = stats.lods;
    }

    fn total_chunks(&self) -> usize {
        self.lods.iter().map(|l| l.chunks).sum()
    }

    fn total_vertices(&self) -> usize {
        self.lods.iter().map(|l| l.vertices).sum()
    }
}

pub fn render(frame: &mut Frame, state: &WorldState, area: Rect) {
    let [summary_area, lods_area] =
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);

    render_summary(frame, state, summary_area);
    render_lods(frame, state, lods_area);
}

/// Encadré du haut : totaux du terrain, dans le style des bandeaux de `main.rs`.
fn render_summary(frame: &mut Frame, state: &WorldState, area: Rect) {
    let (chunks, vertices) = (state.total_chunks(), state.total_vertices());
    let avg = if chunks > 0 { vertices / chunks } else { 0 };

    let line = if state.lods.is_empty() {
        Line::from(Span::styled(
            "  en attente de la génération du terrain…",
            Style::default().fg(MUTED),
        ))
    } else {
        Line::from(vec![
            Span::raw("  "),
            label("CHUNKS"),
            value(fmt_int(chunks)),
            Span::raw("     "),
            label("SOMMETS"),
            value(fmt_int(vertices)),
            Span::raw("     "),
            label("MOYENNE"),
            value(format!("{} /chunk", fmt_int(avg))),
        ])
    };

    let block = Block::default()
        .title(Span::styled("terrain", Style::default().fg(GOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));

    frame.render_widget(Paragraph::new(line).block(block), area);
}

/// Encadré du bas : une ligne par niveau de LOD, plus la ligne de résumé en
/// clair envoyée par le moteur. La barre montre la part du niveau dans le total
/// des sommets — c'est elle qui dit où part réellement le budget géométrie.
fn render_lods(frame: &mut Frame, state: &WorldState, area: Rect) {
    let block = Block::default()
        .title(Span::styled(
            "niveaux de détail  (pas = 1 << LOD ⇒ ~÷pas² sommets)",
            Style::default().fg(GOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG).fg(FG));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.lods.is_empty() {
        let placeholder = Paragraph::new(Span::styled(
            "  aucune donnée de terrain reçue",
            Style::default().fg(MUTED),
        ));
        frame.render_widget(placeholder, inner);
        return;
    }

    // Barre de proportion : largeur adaptative, coupée si l'encadré est étroit.
    let bar_cells = if inner.width >= 70 {
        ((inner.width as usize / 4).clamp(8, 32)).min(inner.width as usize - 62)
    } else {
        0
    };
    let total_verts = state.total_vertices();

    let mut lines = Vec::new();

    // Résumé en clair envoyé par le moteur, en tête du tableau.
    if let Some(summary) = &state.summary {
        lines.push(Line::from(Span::styled(
            format!("  {summary}"),
            Style::default().fg(FG),
        )));
    }

    lines.push(Line::from(Span::styled(
        row("LOD", "pas", "chunks", "somm./chunk", "sommets"),
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    )));

    for l in &state.lods {
        let avg = if l.chunks > 0 { l.vertices / l.chunks } else { 0 };
        let mut spans = vec![Span::raw(row(
            &format!("LOD{}", l.lod),
            &(1u32 << l.lod).to_string(),
            &fmt_int(l.chunks),
            &fmt_int(avg),
            &fmt_int(l.vertices),
        ))];

        if bar_cells > 0 {
            let frac = if total_verts > 0 {
                l.vertices as f64 / total_verts as f64
            } else {
                0.0
            };
            let filled = ((frac * bar_cells as f64).round() as usize).min(bar_cells);
            spans.push(Span::raw("  "));
            spans.push(Span::styled("█".repeat(filled), Style::default().fg(BLUE)));
            spans.push(Span::styled(
                "░".repeat(bar_cells - filled),
                Style::default().fg(MUTED),
            ));
            spans.push(Span::styled(
                format!(" {:>5.1}%", frac * 100.0),
                Style::default().fg(MUTED),
            ));
        }
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Une ligne du tableau : mêmes largeurs pour l'en-tête et les données, valeurs
/// alignées à droite. `{:>n}` compte les caractères, pas les octets — les
/// espaces de milliers de `fmt_int` ne décalent donc rien. `sommets` est la
/// dernière colonne car la barre qui suit en est la proportion.
fn row(lod: &str, step: &str, chunks: &str, avg: &str, vertices: &str) -> String {
    format!("  {lod:<6}{step:>4}{chunks:>12}{avg:>16}{vertices:>16}")
}

/// Sépare les milliers par une espace : « 1 234 567 ». Beaucoup plus lisible
/// qu'un bloc de 7 chiffres quand on compare des ordres de grandeur.
fn fmt_int(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn label(text: &'static str) -> Span<'static> {
    Span::styled(text, Style::default().fg(BLUE).add_modifier(Modifier::BOLD))
}

fn value(text: String) -> Span<'static> {
    Span::styled(format!("  {text}"), Style::default().fg(FG))
}
