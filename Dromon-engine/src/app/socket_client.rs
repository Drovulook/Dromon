use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::Receiver;
use std::time::Duration;

pub const SOCKET_PATH: &str = "/tmp/dromon.sock";

pub fn spawn_worker(receiver: Receiver<String>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut stream: Option<UnixStream> = None;

        loop {
            if stream.is_none() {
                match UnixStream::connect(SOCKET_PATH) {
                    Ok(s) => stream = Some(s),
                    Err(_) => std::thread::sleep(Duration::from_secs(1)),
                }
            }

            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(msg) => {
                    if let Some(ref mut s) = stream {
                        if s.write_all(format!("{}\n", msg).as_bytes()).is_err() {
                            stream = None;
                        }
                    }
                    // si pas connecté, le message est simplement droppé
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    })
}
