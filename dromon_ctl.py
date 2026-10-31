#!/usr/bin/env python3
"""Envoie une commande au moteur et affiche la réponse.

Usage : ./dromon_ctl.py [--app <binaire>] <commande> [clé=valeur ...]
  Sans --app : la seule app lancée (erreur s'il y en a plusieurs).
  ./dromon_ctl.py ping
  ./dromon_ctl.py --app Physis camera.get
  ./dromon_ctl.py camera.get
  ./dromon_ctl.py camera.teleport pos=1200,-340,80 yaw_deg=90
  ./dromon_ctl.py time.pause            (time.resume, time.get)
  ./dromon_ctl.py time.set hour=18.5
  ./dromon_ctl.py screenshot            (PNG temporaire, supprimé à l'arrêt du moteur)

Valeur : JSON si possible, sinon nombres séparés par des virgules (→ liste), sinon texte.
Code de sortie : 0 succès, 1 erreur du moteur ou moteur injoignable, 2 mauvais usage.
"""

import json
import sys

from claude_mcp.dromon_client import engine_socket, request, running_apps


def parse_value(text: str):
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    if "," in text:
        try:
            return [json.loads(part) for part in text.split(",")]
        except json.JSONDecodeError:
            pass
    return text


def parse_args(argv: list[str]) -> tuple[str, dict]:
    """`<commande> clé=valeur…` → (commande, args)."""
    command, *pairs = argv
    args = {}
    for pair in pairs:
        key, sep, value = pair.partition("=")
        if not sep:
            print(f"argument invalide (attendu clé=valeur) : {pair}", file=sys.stderr)
            sys.exit(2)
        args[key] = parse_value(value)
    return command, args


def pick_app(argv: list[str]) -> tuple[str, list[str]]:
    """Extrait `--app <nom>` de `argv`, sinon prend la seule app lancée."""
    if argv[:1] == ["--app"]:
        if len(argv) < 2:
            print("--app : nom du binaire attendu", file=sys.stderr)
            sys.exit(2)
        return argv[1], argv[2:]
    apps = running_apps()
    if len(apps) != 1:
        found = ", ".join(apps) or "aucune"
        print(f"préciser --app (apps lancées : {found})", file=sys.stderr)
        sys.exit(2)
    return apps[0], argv


def main():
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)

    app, argv = pick_app(sys.argv[1:])
    if not argv:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    command, args = parse_args(argv)
    try:
        reply = request(app, command, args)
    except OSError as e:  # socket absent, connexion refusée, timeout
        print(f"moteur injoignable ({engine_socket(app)}) : {e}", file=sys.stderr)
        sys.exit(1)

    if not reply["ok"]:
        print(f"erreur : {reply['error']}", file=sys.stderr)
        sys.exit(1)
    print(json.dumps(reply["result"], indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
