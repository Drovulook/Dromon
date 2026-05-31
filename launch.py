#!/usr/bin/env python3
"""Dromon launcher — édite launch_config.py pour configurer."""

import os
import subprocess
import sys
import time

from launch_config import RELEASE, USE_CLI

SOCKET_PATH = "/tmp/dromon.sock"
WORKSPACE = os.path.dirname(os.path.abspath(__file__))
BUILD_FLAGS = ["--release"] if RELEASE else []


def cargo_run(package: str, extra_args: list[str] | None = None, silent: bool = False) -> subprocess.Popen:
    cmd = ["cargo", "run", "-p", package] + BUILD_FLAGS
    if extra_args:
        cmd += ["--"] + extra_args
    devnull = subprocess.DEVNULL if silent else None
    return subprocess.Popen(cmd, cwd=WORKSPACE, stdout=devnull, stderr=devnull)


def main():
    processes: list[subprocess.Popen] = []

    try:
        if USE_CLI:
            cli = cargo_run("Dromon-cli")
            processes.append(cli)

            print("En attente du socket CLI...", flush=True)
            for _ in range(40):
                if os.path.exists(SOCKET_PATH):
                    break
                time.sleep(0.25)
            else:
                print("Erreur : le CLI n'a pas créé le socket.", file=sys.stderr)
                cli.terminate()
                sys.exit(1)

            engine = cargo_run("Dromon-engine", ["--use-cli"], silent=True)
            processes.append(engine)
        else:
            processes.append(cargo_run("Dromon-engine"))

        while all(p.poll() is None for p in processes):
            time.sleep(0.2)

    except KeyboardInterrupt:
        pass

    finally:
        for p in processes:
            p.terminate()
        for p in processes:
            p.wait()
        subprocess.run(["stty", "sane"], check=False)


if __name__ == "__main__":
    main()
