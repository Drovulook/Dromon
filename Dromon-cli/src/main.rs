mod app_state;
mod log_parser;
mod tabs;

use app_state::{AppState, Tab};
use color_eyre::eyre::Result;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyModifiers},
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};
use std::{
    io::{BufRead, BufReader},
    os::unix::net::UnixListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

const SOCKET_PATH: &str = "/tmp/dromon.sock";

fn main() -> Result<()> {
    color_eyre::install()?;

    let shutdown = Arc::new(AtomicBool::new(false));

    let shutdown_signal = shutdown.clone();
    ctrlc::set_handler(move || {
        shutdown_signal.store(true, Ordering::SeqCst);
    })?;

    let _ = std::fs::remove_file(SOCKET_PATH);
    let listener = UnixListener::bind(SOCKET_PATH)?;

    let (tx, rx) = mpsc::channel::<String>();

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stream);
                for line in reader.lines().flatten() {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
    });

    let mut state = AppState::new(rx, shutdown);
    let terminal = ratatui::init();
    let result = run(terminal, &mut state);
    ratatui::restore();

    let _ = std::fs::remove_file(SOCKET_PATH);
    result
}

fn run(mut terminal: DefaultTerminal, state: &mut AppState) -> Result<()> {
    loop {
        if state.shutdown.load(Ordering::SeqCst) {
            break;
        }

        while let Ok(msg) = state.rx.try_recv() {
            state.push(msg);
        }

        terminal.draw(|frame| render(frame, state))?;

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    // Raccourcis globaux : quitter et changer d'onglet.
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Left if ctrl => state.prev_tab(),
                    KeyCode::Right if ctrl => state.next_tab(),
                    // Les autres touches dépendent de l'onglet actif.
                    _ => match state.active_tab {
                        Tab::Logs => match key.code {
                            KeyCode::Up => state.logs.scroll_up(),
                            KeyCode::Down => state.logs.scroll_down(),
                            KeyCode::Char('g') | KeyCode::Home => state.logs.go_to_top(),
                            KeyCode::Char('G') | KeyCode::End => state.logs.go_to_bottom(),
                            _ => (),
                        },
                        Tab::Profiling => match key.code {
                            KeyCode::Up => state.profiling.move_up(),
                            KeyCode::Down => state.profiling.move_down(),
                            KeyCode::Left => state.profiling.move_left(),
                            KeyCode::Right => state.profiling.move_right(),
                            KeyCode::Char('z') => state.profiling.toggle_zoom(),
                            _ => (),
                        },
                        Tab::World => (),
                    },
                }
            }
        }
    }
    Ok(())
}

const BG: Color = Color::Rgb(0x02, 0x00, 0x02);
const BORDER: Color = Color::Rgb(0x9D, 0x7B, 0xBF);
const GOLD: Color = Color::Rgb(0xE6, 0xA2, 0x4C);
const FG: Color = Color::Rgb(0xBF, 0xBD, 0xB6);
const BLUE: Color = Color::Rgb(0x56, 0x9C, 0xD6);
const MUTED: Color = Color::Rgb(0x6B, 0x6A, 0x66);

fn render(frame: &mut Frame, state: &mut AppState) {
    let [top_area, tabs_area, content_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(frame.area());

    // Barre du haut : statut (STATE/FPS) à gauche, config (BUILD/PROFILING) à droite.
    let [status_area, config_area] =
        Layout::horizontal([Constraint::Length(134), Constraint::Min(0)]).areas(top_area);

    render_status(frame, state, status_area);
    render_config(frame, state, config_area);
    render_tabs(frame, state, tabs_area);

    // Chaque onglet dessine son contenu à partir de son propre sous-état.
    match state.active_tab {
        Tab::Logs => tabs::logs::render(frame, &mut state.logs, content_area),
        Tab::Profiling => tabs::profiling::render(frame, &state.profiling, content_area),
        Tab::World => tabs::world::render(frame, content_area),
    }
}

fn render_tabs(frame: &mut Frame, state: &AppState, area: ratatui::layout::Rect) {
    // Petite marge à gauche, onglets au centre, rappel de navigation à droite.
    let [_, tabs_area, hint_area] = Layout::horizontal([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(26),
    ])
    .areas(area);

    // Onglet actif : « chip » à fond plein ; inactifs atténués. Le padding donne
    // de l'air autour des libellés et fait ressortir le fond du chip.
    let tabs = Tabs::new(Tab::ALL.iter().map(|t| t.title()))
        .select(state.active_tab.index())
        .style(Style::default().fg(MUTED))
        .highlight_style(
            Style::default()
                .fg(BG)
                .bg(GOLD)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled("·", Style::default().fg(BORDER)))
        .padding(" ", " ");
    frame.render_widget(tabs, tabs_area);

    let hint = Paragraph::new(Span::styled(
        "Ctrl+←/→ onglet ",
        Style::default().fg(BORDER),
    ))
    .alignment(Alignment::Right);
    frame.render_widget(hint, hint_area);
}

fn render_status(frame: &mut Frame, state: &AppState, area: ratatui::layout::Rect) {
    let engine_state = state.state.as_deref().unwrap_or("—");
    let fps_str = state
        .fps
        .map(|f| format!("{:.1}", f))
        .unwrap_or_else(|| "—".to_string());

    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "STATE",
            Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(engine_state, Style::default().fg(FG)),
        Span::raw("     "),
        Span::styled(
            "FPS",
            Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(fps_str, Style::default().fg(FG)),
    ]);

    let block = Block::default()
        .title(Span::styled("Dromon Engine", Style::default().fg(GOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));

    frame.render_widget(Paragraph::new(line).block(block), area);
}

fn render_config(frame: &mut Frame, state: &AppState, area: ratatui::layout::Rect) {
    let build = state.config_mode.as_deref().unwrap_or("—");
    let profiling = match state.config_profiling {
        Some(true) => "enabled",
        Some(false) => "disabled",
        None => "—",
    };

    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "BUILD",
            Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(build.to_string(), Style::default().fg(FG)),
        Span::raw("     "),
        Span::styled(
            "PROFILING",
            Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(profiling, Style::default().fg(FG)),
    ]);

    let block = Block::default()
        .title(Span::styled("config", Style::default().fg(GOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));

    frame.render_widget(Paragraph::new(line).block(block), area);
}
