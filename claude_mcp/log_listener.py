"""Écoute en continu les logs diffusés par le moteur et garde les derniers, pour l'outil
`logs_tail` du pont MCP. Se reconnecte seul quand le moteur est (re)lancé.

Limite : seuls les logs émis pendant la connexion sont vus (pas ceux d'un démarrage
antérieur au pont).
"""

import collections
import socket
import threading
import time

from dromon_client import engine_socket

# Données destinées au TUI (graphes, arbres de profiling) : du bruit pour l'agent.
IGNORED_TAGS = ("[FPS]", "[PROFILE_", "[WORLD]", "[STATE]", "[REPLY]")
RETRY_SECS = 1.0

# Niveau demandé → tags retenus (`None` : tous). Les messages des validation layers
# comptent comme des avertissements ; les événements du pont sont toujours montrés.
LEVELS = {
    "INFO": None,
    "WARN": ("[WARN]", "[ERROR]", "[VULKAN]", "[BRIDGE]"),
    "ERROR": ("[ERROR]", "[BRIDGE]"),
}


def line_tag(line: str, previous: str) -> str:
    """`[TAG]` en tête de ligne ; sans tag, suite du message précédent (panique sur
    plusieurs lignes…) : même tag, pour que le filtre par niveau ne la perde pas."""
    if line.startswith("["):
        end = line.find("]")
        if end > 0:
            return line[: end + 1]
    return previous


class LogListener:
    def __init__(self, app: str, capacity: int = 500):
        self.app = app
        # (heure de réception, tag, ligne) ; les plus anciennes tombent une fois plein.
        self._entries = collections.deque(maxlen=capacity)
        # Le thread d'écoute écrit pendant que `tail` lit : parcourir une deque
        # modifiée en même temps lève une erreur.
        self._lock = threading.Lock()

    def start(self):
        # `daemon` : ne retient pas la fin du processus (boucle infinie).
        threading.Thread(target=self._run, name="logs", daemon=True).start()

    def tail(self, n: int, level: str) -> list[str]:
        """Les `n` dernières lignes de niveau ≥ `level`, les plus récentes en dernier."""
        tags = LEVELS[level]
        with self._lock:
            entries = list(self._entries)
        if tags is not None:
            entries = [e for e in entries if e[1].startswith(tags)]
        return [f"{stamp} {line}" for stamp, _, line in entries[-n:]]

    def _push(self, tag: str, line: str):
        with self._lock:
            self._entries.append((time.strftime("%H:%M:%S"), tag, line))

    def _run(self):
        while True:
            connected = False
            try:
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
                    sock.connect(str(engine_socket(self.app)))
                    connected = True
                    self._push("[BRIDGE]", "[BRIDGE] connecté au moteur")
                    tag = ""
                    for raw in sock.makefile("r", encoding="utf-8", errors="replace"):
                        line = raw.rstrip("\n")
                        tag = line_tag(line, tag)
                        if not tag.startswith(IGNORED_TAGS):
                            self._push(tag, line)
            except OSError:
                pass  # moteur absent (connexion refusée) ou coupure brutale
            if connected:
                self._push("[BRIDGE]", "[BRIDGE] connexion au moteur perdue (arrêt ou crash)")
            time.sleep(RETRY_SECS)
