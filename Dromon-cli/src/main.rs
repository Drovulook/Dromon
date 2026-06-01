mod app_state;

use app_state::AppState;
use color_eyre::eyre::Result;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem},
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

fn render(frame: &mut Frame, state: &mut AppState) {
    state.viewport_height = (frame.area().height as usize).saturating_sub(2).max(1);
    if state.auto_scroll {
        *state.list_state.offset_mut() = state.bottom_offset();
    }

    let items: Vec<ListItem> = state
        .logs
        .iter()
        .map(|l| ListItem::new(l.as_str()))
        .collect();

    let title = if state.auto_scroll {
        "Dromon Logs — (↑↓ scroll  g/G haut/bas  q quitter)"
    } else {
        "Dromon Logs — [scroll manuel] (G = bas + auto)"
    };

    const BG: Color = Color::Rgb(0x07, 0x00, 0x00);
    const BORDER: Color = Color::Rgb(0x1E, 0x2D, 0x3D);
    const TITLE: Color = Color::Rgb(0xAA, 0x22, 0x22);
    const FG: Color = Color::Rgb(0xBF, 0xBD, 0xB6);

    let block = Block::default()
        .title(ratatui::text::Span::styled(
            title,
            Style::default().fg(TITLE),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG).fg(FG));

    frame.render_stateful_widget(
        List::new(items).block(block),
        frame.area(),
        &mut state.list_state,
    );
}
