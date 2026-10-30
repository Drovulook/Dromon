//! Serveur du socket : le moteur écoute sur [`socket_path`], diffuse les logs à tous
//! les clients et transmet leurs lignes (commandes) à l'`Engine`, avec de quoi répondre
//! au seul client qui a demandé.

use std::io::{self, BufRead, BufReader, ErrorKind, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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

/// Socket du moteur pour l'app `app` : `$XDG_RUNTIME_DIR/dromon/<app>.sock` (tmpfs propre
/// à l'utilisateur), sinon dans le dossier temporaire. ⚠ Même règle dans le CLI et
/// `claude_mcp/dromon_client.py`.
pub fn socket_path(app: &str) -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("dromon").join(format!("{app}.sock"))
}

pub struct ServerHandles {
    pub worker: JoinHandle<()>,
    /// Logs et réponses vers le socket.
    pub outgoing: SyncSender<Outgoing>,
    /// Lignes reçues des clients.
    pub incoming: Receiver<Incoming>,
    /// Un client s'est connecté pendant l'attente du démarrage.
    pub client_connected: bool,
}

/// Ouvre le socket `path`. `wait_first_client` : attend un client ([`FIRST_CLIENT_TIMEOUT`]
/// au plus) avant de rendre la main. Erreur si une autre instance de l'app l'occupe.
pub fn spawn_worker(path: PathBuf, wait_first_client: bool) -> io::Result<ServerHandles> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Ne pas voler le socket d'une instance vivante (elle ne recevrait plus rien).
    if UnixStream::connect(&path).is_ok() {
        return Err(io::Error::new(
            ErrorKind::AddrInUse,
            format!("{} occupé par une autre instance", path.display()),
        ));
    }
    // Fichier laissé par un moteur tué : `bind` échouerait.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    // `accept` non bloquant : le worker alterne entre nouveaux clients et messages.
    listener.set_nonblocking(true)?;

    let (outgoing_tx, outgoing_rx) = sync_channel(OUTGOING_CAPACITY);
    let (incoming_tx, incoming_rx) = channel();
    let mut server = Server {
        path,
        listener,
        clients: Vec::new(),
        next_id: 0,
        outgoing: outgoing_tx.clone(),
        incoming: incoming_tx,
    };
    if wait_first_client {
        let deadline = Instant::now() + FIRST_CLIENT_TIMEOUT;
        while server.clients.is_empty() && Instant::now() < deadline {
            server.accept_clients();
            std::thread::sleep(POLL);
        }
    }
    let client_connected = !server.clients.is_empty();

    let worker = std::thread::spawn(move || server.run(outgoing_rx));
    Ok(ServerHandles {
        worker,
        outgoing: outgoing_tx,
        incoming: incoming_rx,
        client_connected,
    })
}

struct Server {
    path: PathBuf,
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
        let _ = std::fs::remove_file(&self.path);
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
