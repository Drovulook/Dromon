#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["mcp>=2.3,<3"]
# ///
"""Pont MCP : présente le moteur Dromon à Claude comme une liste d'outils.

Lancé par Claude Code (cf. `.mcp.json` de l'app), il dialogue en JSON-RPC sur
stdin/stdout et relaie chaque outil en commande sur le socket du moteur.
⚠ Ne jamais écrire sur stdout (`print`) : c'est le canal du protocole. Diagnostics →
stderr.

Usage : uv run --script claude_mcp/dromon_mcp.py --app <binaire>
"""

import argparse
import time
from typing import Any, Literal

from mcp.server.mcpserver import Image, MCPServer
from mcp.server.mcpserver.exceptions import ToolError

from dromon_client import request
from log_listener import LogListener

INSTRUCTIONS = """\
Pilote le moteur de jeu Dromon en cours d'exécution (lancé par l'utilisateur, pas par
ces outils). Conventions : monde Z-up (+Z = haut), positions en voxels ; yaw 0° = +X,
90° = +Y ; pitch > 0 = regard vers le haut. La caméra est bornée au disque du monde.
Si un outil répond « moteur injoignable », demander à l'utilisateur de (re)lancer l'app.
"""

mcp = MCPServer("dromon", instructions=INSTRUCTIONS)

# Binaire ciblé (`--app`) et écoute de ses logs, fixés dans `main` avant de servir.
APP = ""
LOGS: LogListener | None = None


def call(cmd: str, **args: Any) -> Any:
    """Envoie `cmd` (les args à `None` sont omis : valeur actuelle conservée) et rend le
    résultat. Échec → `ToolError`, que l'agent reçoit comme message d'erreur."""
    args = {key: value for key, value in args.items() if value is not None}
    try:
        reply = request(APP, cmd, args)
    except OSError as e:  # socket absent, connexion refusée, pas de réponse
        raise ToolError(f"moteur « {APP} » injoignable ou figé : {e}") from e
    if not reply["ok"]:
        raise ToolError(reply["error"])
    return reply["result"]


# ─── Outils ──────────────────────────────────────────────────────────────────────
# La docstring est la description lue par l'agent ; les annotations de type donnent
# le schéma des paramètres.


@mcp.tool()
def ping() -> str:
    """Vérifie que le moteur répond ("pong")."""
    return call("ping")


@mcp.tool()
def camera_get() -> dict[str, Any]:
    """Pose de la caméra : pos [x, y, z], yaw_deg dans [0, 360[, pitch_deg, fov_deg."""
    return call("camera.get")


@mcp.tool()
def camera_teleport(
    pos: tuple[float, float, float],
    yaw_deg: float | None = None,
    pitch_deg: float | None = None,
) -> dict[str, Any]:
    """Déplace la caméra en `pos` [x, y, z] et l'oriente (angles omis : inchangés).
    Refusé hors du disque du monde. Rend la nouvelle pose."""
    return call("camera.teleport", pos=pos, yaw_deg=yaw_deg, pitch_deg=pitch_deg)


@mcp.tool()
def time_get() -> dict[str, Any]:
    """Horloge du jeu : day, hour dans [0, 24), paused, time_scale (0 = figée)."""
    return call("time.get")


@mcp.tool()
def time_pause() -> dict[str, Any]:
    """Fige le temps : soleil, vagues et étoiles (la caméra reste mobile)."""
    return call("time.pause")


@mcp.tool()
def time_resume() -> dict[str, Any]:
    """Relance le temps à sa vitesse d'avant la pause."""
    return call("time.resume")


@mcp.tool()
def time_set(hour: float) -> dict[str, Any]:
    """Règle l'heure dans [0, 24) (même jour). Rend l'état de l'horloge."""
    return call("time.set", hour=hour)


@mcp.tool()
def screenshot(wait_s: float = 0.0) -> list:
    """Capture la fenêtre du jeu : l'image, et le chemin du PNG (temporaire, supprimé à
    l'arrêt du moteur). `wait_s` (0–30) : attente avant la capture, le temps que le
    terrain se charge après un déplacement ou une régénération (quelques secondes).
    Coûte ~1 500–2 000 tokens : n'en prendre que pour juger un changement visible."""
    time.sleep(min(max(wait_s, 0.0), 30.0))
    path = call("screenshot")["path"]
    return [f"capture : {path}", Image(path=path)]


@mcp.tool()
def logs_tail(n: int = 50, level: Literal["INFO", "WARN", "ERROR"] = "INFO") -> str:
    """Derniers logs du moteur (les plus récents en dernier), horodatés à la réception.
    level : INFO = tout ; WARN = avertissements, erreurs et validation Vulkan ;
    ERROR = erreurs seules. Les lignes [BRIDGE] signalent connexion et perte du moteur.
    Après une modification d'un fichier de config, vérifier ici que le rechargement a
    réussi. Ne contient que les logs reçus depuis le démarrage de ce pont."""
    lines = LOGS.tail(max(n, 1), level)
    return "\n".join(lines) or "(aucun log reçu à ce niveau)"


def main():
    global APP, LOGS
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--app", required=True, help="binaire ciblé (ex. ai_sandbox)")
    APP = parser.parse_args().app
    LOGS = LogListener(APP)
    LOGS.start()
    mcp.run("stdio")


if __name__ == "__main__":
    main()
