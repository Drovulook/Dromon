mod app_state;
mod log_parser;

use app_state::AppState;
use color_eyre::eyre::Result;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
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
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Up => state.scroll_up(),
                    KeyCode::Down => state.scroll_down(),
                    KeyCode::Char('g') | KeyCode::Home => state.go_to_top(),
                    KeyCode::Char('G') | KeyCode::End => state.go_to_bottom(),
                    _ => (),
                }
            }
        }
    }
    Ok(())
}

const BG: Color = Color::Rgb(0x02, 0x00, 0x02);
const BORDER: Color = Color::Rgb(0x9D, 0x7B, 0xBF);
const TITLE: Color = Color::Rgb(0xE6, 0xA2, 0x4C);
const FG: Color = Color::Rgb(0xBF, 0xBD, 0xB6);
const ACCENT: Color = Color::Rgb(0x56, 0x9C, 0xD6);

fn render(frame: &mut Frame, state: &mut AppState) {
    let [status_area, logs_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .areas(frame.area());

    state.viewport_height = (logs_area.height as usize).saturating_sub(2).max(1);
    if state.auto_scroll {
        *state.list_state.offset_mut() = state.bottom_offset();
    }

    render_status(frame, state, status_area);
    render_logs(frame, state, logs_area);
}

fn render_status(frame: &mut Frame, state: &AppState, area: ratatui::layout::Rect) {
    let engine_state = state.state.as_deref().unwrap_or("—");
    let fps_str = state
        .fps
        .map(|f| format!("{:.1}", f))
        .unwrap_or_else(|| "—".to_string());

    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled("STATE", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(engine_state, Style::default().fg(FG)),
        Span::raw("     "),
        Span::styled("FPS", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(fps_str, Style::default().fg(FG)),
    ]);

    let block = Block::default()
        .title(Span::styled("Dromon Engine", Style::default().fg(TITLE)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));

    frame.render_widget(Paragraph::new(line).block(block), area);
}

fn render_logs(frame: &mut Frame, state: &mut AppState, area: ratatui::layout::Rect) {
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

    frame.render_stateful_widget(
        List::new(items).block(block),
        area,
        &mut state.list_state,
    );
}
