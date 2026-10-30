"""Client du moteur Dromon : chemins des sockets (un par app = nom du binaire) et envoi
d'une commande. Utilisé par `dromon_ctl.py`, `launch.py` et le pont MCP.

⚠ Chemins : même règle que `socket_path` (Dromon-engine/src/app/socket_server.rs) et
`socket_dir` (Dromon-cli/src/main.rs).
"""

import itertools
import json
import os
import socket
import tempfile
import time
from pathlib import Path

TIMEOUT_SECS = 5.0
REPLY_TAG = "[REPLY] "

# Unicité des `id` au sein d'un processus (le pont MCP envoie plusieurs requêtes).
_ids = itertools.count(1)


def socket_dir() -> Path:
    base = os.environ.get("XDG_RUNTIME_DIR") or tempfile.gettempdir()
    return Path(base) / "dromon"


def engine_socket(app: str) -> Path:
    """Servi par le moteur : logs diffusés + commandes."""
    return socket_dir() / f"{app}.sock"


def cli_socket(app: str) -> Path:
    """Servi par le CLI : le launcher y envoie la compilation et l'état du moteur."""
    return socket_dir() / f"{app}-cli.sock"


def running_apps() -> list[str]:
    """Apps dont le socket moteur existe. Peut inclure un moteur tué (socket resté)."""
    directory = socket_dir()
    if not directory.is_dir():
        return []
    return sorted(
        p.stem for p in directory.glob("*.sock") if not p.stem.endswith("-cli")
    )


def request(app: str, cmd: str, args: dict | None = None) -> dict:
    """Envoie `cmd` au moteur de `app` et rend sa réponse :
    `{"ok": True, "result": …}` ou `{"ok": False, "error": "…"}`.
    `OSError` si le moteur est injoignable, `TimeoutError` (sous-classe) sans réponse.

    Le moteur diffuse aussi ses logs à tous les clients : on ignore tout ce qui n'est
    pas notre `[REPLY]`.
    """
    req = {"id": next(_ids), "cmd": cmd}
    if args:
        req["args"] = args
    deadline = time.monotonic() + TIMEOUT_SECS
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(TIMEOUT_SECS)
        sock.connect(str(engine_socket(app)))
        sock.sendall((json.dumps(req) + "\n").encode())
        for line in sock.makefile("r", encoding="utf-8"):
            if time.monotonic() > deadline:
                break
            if not line.startswith(REPLY_TAG):
                continue
            reply = json.loads(line[len(REPLY_TAG) :])
            if reply.get("id") == req["id"]:
                return reply
    raise TimeoutError(f"pas de réponse en {TIMEOUT_SECS} s")
