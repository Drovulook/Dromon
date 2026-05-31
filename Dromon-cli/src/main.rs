use color_eyre::eyre::Result;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode},
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

struct AppState {
    logs: Vec<String>,
    rx: mpsc::Receiver<String>,
    shutdown: Arc<AtomicBool>,
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let shutdown = Arc::new(AtomicBool::new(false));

    // Intercepte SIGTERM (launch.py) et SIGINT (Ctrl+C)
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

    let mut state = AppState { logs: Vec::new(), rx, shutdown };
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
            state.logs.push(msg);
        }

        terminal.draw(|frame| render(frame, state))?;

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    _ => (),
                }
            }
        }
    }
    Ok(())
}

fn render(frame: &mut Frame, state: &AppState) {
    let items: Vec<ListItem> = state
        .logs
        .iter()
        .map(|l| ListItem::new(l.as_str()))
        .collect();
    frame.render_widget(
        List::new(items).block(Block::default().title("Dromon Logs").borders(Borders::ALL)),
        frame.area(),
    );
}
