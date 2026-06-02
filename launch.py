#!/usr/bin/env python3
"""Dromon launcher — édite launch_config.py pour configurer."""

import os
import socket
import subprocess
import sys
import time

from launch_config import RELEASE, USE_CLI

SOCKET_PATH = "/tmp/dromon.sock"
WORKSPACE = os.path.dirname(os.path.abspath(__file__))
PROFILE = "release" if RELEASE else "debug"
BUILD_FLAGS = ["--release"] if RELEASE else []


def cargo_build(package: str) -> tuple[bool, str]:
    cmd = ["cargo", "build", "-p", package] + BUILD_FLAGS
    result = subprocess.run(cmd, cwd=WORKSPACE, capture_output=True, text=True)
    return result.returncode == 0, result.stderr


def send_to_cli(text: str):
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
            s.connect(SOCKET_PATH)
            for line in text.splitlines():
                if line.strip():
                    s.sendall((line + "\n").encode())
    except Exception:
        pass


def run_binary(
    package: str, extra_args: list[str] | None = None, silent: bool = False
) -> subprocess.Popen:
    binary = os.path.join(WORKSPACE, "target", PROFILE, package)
    cmd = [binary] + (extra_args or [])
    devnull = subprocess.DEVNULL if silent else None
    return subprocess.Popen(cmd, cwd=WORKSPACE, stdout=devnull, stderr=devnull)


def cargo_run(
    package: str, extra_args: list[str] | None = None, silent: bool = False
) -> subprocess.Popen:
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

            for _ in range(40):
                if os.path.exists(SOCKET_PATH):
                    break
                time.sleep(0.25)
            else:
                sys.exit(1)

            success, build_output = cargo_build("Dromon-engine")
            block = "--------------------------------------------------------------------- COMPILATION ---------------------------------------------------------------------\n"
            if build_output.strip():
                block += build_output
            block += "-------------------------------------------------------------------------------------------------------------------------------------------------------\n"
            send_to_cli(block)

            if not success:
                cli.wait()  # attend que l'utilisateur ferme le CLI (q / Esc)
                sys.exit(1)

            engine = run_binary("Dromon-engine", ["--use-cli"])
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
