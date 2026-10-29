//! Serveur du socket : le moteur écoute sur [`SOCKET_PATH`], diffuse les logs à tous
//! les clients et transmet leurs lignes (commandes) à l'`Engine`, avec de quoi répondre
//! au seul client qui a demandé.

use std::io::{self, BufRead, BufReader, ErrorKind, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const SOCKET_PATH: &str = "/tmp/dromon.sock";

/// Attente du CLI au démarrage. Le moteur ne log rien avant sa connexion : aucun
/// message n'est perdu, même sur un crash brutal (segfault du driver).
const FIRST_CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(10);
const RECV_TIMEOUT: Duration = Duration::from_millis(50);
/// Client qui ne lit plus (suspendu, bloqué) : abandonné au-delà, sinon il bloquerait
/// le worker, puis le moteur dès qu'une réponse attend une place dans le channel.
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);
/// File bornée : si le socket ne suit plus, les logs sont jetés (`try_send`) plutôt que
/// d'accumuler de la mémoire ou de bloquer le rendu.
const OUTGOING_CAPACITY: usize = 1024;

type ClientId = u64;

/// Ce que le worker envoie sur le socket.
pub enum Outgoing {
    Broadcast(String),
    Reply { client: ClientId, line: String },
    /// Fin explicite : le worker garde un émetteur (pour les [`Replier`]), le channel
    /// ne se déconnecte donc jamais tout seul.
    Shutdown,
}

/// Ligne reçue d'un client.
pub struct Incoming {
    pub line: String,
    pub reply: Replier,
}

/// Répond au client d'origine uniquement.
#[derive(Clone)]
pub struct Replier {
    client: ClientId,
    outgoing: SyncSender<Outgoing>,
}

impl Replier {
    pub fn send(&self, line: String) {
        // Bloquant (contrairement aux logs) : une réponse ne doit pas être perdue.
        let _ = self.outgoing.send(Outgoing::Reply {
            client: self.client,
            line,
        });
    }
}

/// Bloque jusqu'à la connexion du premier client ; erreur `TimedOut` sinon. Renvoie le
/// worker, l'émetteur vers le socket et les lignes reçues des clients.
pub fn spawn_worker() -> io::Result<(JoinHandle<()>, SyncSender<Outgoing>, Receiver<Incoming>)> {
    // Fichier laissé par un moteur tué : `bind` échouerait.
    let _ = std::fs::remove_file(SOCKET_PATH);
    let listener = UnixListener::bind(SOCKET_PATH)?;
    // `accept` non bloquant : le worker alterne entre nouveaux clients et messages.
    listener.set_nonblocking(true)?;

    let (outgoing_tx, outgoing_rx) = sync_channel(OUTGOING_CAPACITY);
    let (incoming_tx, incoming_rx) = channel();
    let deadline = Instant::now() + FIRST_CLIENT_TIMEOUT;
    let mut server = Server {
        listener,
        clients: Vec::new(),
        next_id: 0,
        outgoing: outgoing_tx.clone(),
        incoming: incoming_tx,
    };
    while server.clients.is_empty() {
        if Instant::now() >= deadline {
            let _ = std::fs::remove_file(SOCKET_PATH);
            return Err(io::Error::new(
                ErrorKind::TimedOut,
                "aucun client sur le socket des logs",
            ));
        }
        server.accept_clients();
        std::thread::sleep(POLL);
    }

    let worker = std::thread::spawn(move || server.run(outgoing_rx));
    Ok((worker, outgoing_tx, incoming_rx))
}

struct Server {
    listener: UnixListener,
    clients: Vec<(ClientId, UnixStream)>,
    next_id: ClientId,
    outgoing: SyncSender<Outgoing>,
    incoming: Sender<Incoming>,
}

impl Server {
    fn run(mut self, receiver: Receiver<Outgoing>) {
        loop {
            self.accept_clients();
            match receiver.recv_timeout(RECV_TIMEOUT) {
                Ok(Outgoing::Broadcast(msg)) => self.write(None, &msg),
                Ok(Outgoing::Reply { client, line }) => self.write(Some(client), &line),
                Ok(Outgoing::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
        let _ = std::fs::remove_file(SOCKET_PATH);
    }

    fn accept_clients(&mut self) {
        while let Ok((stream, _)) = self.listener.accept() {
            // Écritures bloquantes : `write_all` ne doit pas s'arrêter en plein message.
            if stream.set_nonblocking(false).is_err()
                || stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_err()
            {
                continue;
            }
            let Ok(read_half) = stream.try_clone() else {
                continue;
            };
            let id = self.next_id;
            self.next_id += 1;
            self.spawn_reader(id, read_half);
            self.clients.push((id, stream));
        }
    }

    /// Un thread de lecture par client (lecture bloquante) ; il s'arrête à la déconnexion.
    fn spawn_reader(&self, client: ClientId, stream: UnixStream) {
        let incoming = self.incoming.clone();
        let reply = Replier {
            client,
            outgoing: self.outgoing.clone(),
        };
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let request = Incoming {
                    line,
                    reply: reply.clone(),
                };
                if incoming.send(request).is_err() {
                    return;
                }
            }
        });
    }

    /// `to` : `None` → tous les clients. Sans client, le message est perdu.
    /// Écriture en échec (déconnexion → `EPIPE`, Rust ignore `SIGPIPE` ; ou
    /// [`WRITE_TIMEOUT`]) → client retiré.
    fn write(&mut self, to: Option<ClientId>, msg: &str) {
        let line = format!("{msg}\n");
        self.clients.retain_mut(|(id, client)| {
            if to.is_some_and(|to| to != *id) || client.write_all(line.as_bytes()).is_ok() {
                return true;
            }
            // Le clone du thread de lecture garde le socket ouvert : `shutdown` le ferme
            // pour de bon, le client voit la fin et le thread de lecture s'arrête.
            let _ = client.shutdown(Shutdown::Both);
            false
        });
    }
}
