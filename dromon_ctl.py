#!/usr/bin/env python3
"""Envoie une commande au moteur (lancé avec --use-cli) et affiche la réponse.

Usage : ./dromon_ctl.py <commande> [clé=valeur ...]
  ./dromon_ctl.py ping
  ./dromon_ctl.py camera.get
  ./dromon_ctl.py camera.teleport pos=1200,-340,80 yaw_deg=90
  ./dromon_ctl.py time.pause            (time.resume, time.get)
  ./dromon_ctl.py time.set hour=18.5

Valeur : JSON si possible, sinon nombres séparés par des virgules (→ liste), sinon texte.
Code de sortie : 0 succès, 1 erreur du moteur ou moteur injoignable, 2 mauvais usage.
"""

import json
import os
import socket
import sys
import time

SOCKET_PATH = "/tmp/dromon.sock"
TIMEOUT_SECS = 5.0
REPLY_TAG = "[REPLY] "


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


def parse_args(argv: list[str]) -> dict:
    command, *pairs = argv
    request = {"id": os.getpid(), "cmd": command}
    args = {}
    for pair in pairs:
        key, sep, value = pair.partition("=")
        if not sep:
            print(f"argument invalide (attendu clé=valeur) : {pair}", file=sys.stderr)
            sys.exit(2)
        args[key] = parse_value(value)
    if args:
        request["args"] = args
    return request


def send(request: dict) -> dict:
    """Envoie la requête et attend sa réponse. Le moteur diffuse aussi ses logs à
    tous les clients : on ignore tout ce qui n'est pas notre `[REPLY]`."""
    deadline = time.monotonic() + TIMEOUT_SECS
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(TIMEOUT_SECS)
        sock.connect(SOCKET_PATH)
        sock.sendall((json.dumps(request) + "\n").encode())
        for line in sock.makefile("r", encoding="utf-8"):
            if time.monotonic() > deadline:
                break
            if not line.startswith(REPLY_TAG):
                continue
            reply = json.loads(line[len(REPLY_TAG):])
            if reply.get("id") == request["id"]:
                return reply
    raise TimeoutError(f"pas de réponse en {TIMEOUT_SECS} s")


def main():
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)

    try:
        reply = send(parse_args(sys.argv[1:]))
    except OSError as e:  # socket absent, connexion refusée, timeout
        print(f"moteur injoignable ({SOCKET_PATH}) : {e}", file=sys.stderr)
        sys.exit(1)

    if not reply["ok"]:
        print(f"erreur : {reply['error']}", file=sys.stderr)
        sys.exit(1)
    print(json.dumps(reply["result"], indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
