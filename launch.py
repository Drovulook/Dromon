#!/usr/bin/env python3
"""Dromon launcher — édite launch_config.py pour configurer."""

import os
import socket
import subprocess
import sys
import time

from launch_config import ENABLE_PROFILING, RELEASE, USE_CLI

SOCKET_PATH = "/tmp/dromon.sock"
WORKSPACE = os.path.dirname(os.path.abspath(__file__))
PROFILE = "release" if RELEASE else "debug"
BUILD_FLAGS = ["--release"] if RELEASE else []


# Feature Cargo de l'engine, activée seulement si le profiling est demandé. La
# passer au build de l'app compile l'instrumentation ; l'omettre l'efface du binaire.
ENGINE_PROFILING_FEATURE = ["--features", "Dromon-engine/profiling"]


def cargo_build(package: str, features: list[str] | None = None) -> tuple[bool, str]:
    cmd = ["cargo", "build", "-p", package] + BUILD_FLAGS + (features or [])
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


def cargo_run(
    package: str,
    extra_args: list[str] | None = None,
    silent: bool = False,
    features: list[str] | None = None,
) -> subprocess.Popen:
    cmd = ["cargo", "run", "-p", package] + BUILD_FLAGS + (features or [])
    if extra_args:
        cmd += ["--"] + extra_args
    devnull = subprocess.DEVNULL if silent else None
    return subprocess.Popen(cmd, cwd=WORKSPACE, stdout=devnull, stderr=devnull)


def engine_args() -> list[str]:
    """Arguments runtime passés au binaire de l'app.

    --use-cli branche les logs sur le socket du CLI ; --use-profiling signale à la
    librairie (engine) d'armer son instrumentation. Le launcher ne fait que
    transmettre ces flags selon la config.
    """
    args = []
    if USE_CLI:
        args.append("--use-cli")
    if ENABLE_PROFILING:
        args.append("--use-profiling")
    return args


def main():
    processes: list[subprocess.Popen] = []

    # Package/binaire à lancer : tout argument `--<nom>` le sélectionne, ex.
    # `./launch.py --model_sandbox`. Défaut : model_sandbox.
    target = "model_sandbox"
    for arg in sys.argv[1:]:
        if arg.startswith("--"):
            target = arg[2:]

    # Un seul interrupteur : ENABLE_PROFILING active à la fois la feature Cargo
    # (compile l'instrumentation) et le flag runtime `--use-profiling`.
    target_features = ENGINE_PROFILING_FEATURE if ENABLE_PROFILING else []

    try:
        if USE_CLI:
            # On compile le CLI AVANT de le lancer. Sinon `cargo run` compile les
            # dépendances pendant qu'on attend le socket : une recompilation longue
            # dépasse le timeout, le launcher abandonne, et le CLI démarre vide sans
            # que l'app ne se lance.
            print("Compilation du CLI…")
            success, build_output = cargo_build("Dromon-cli")
            if not success:
                print(build_output, file=sys.stderr)
                sys.exit(1)

            cli = cargo_run("Dromon-cli")
            processes.append(cli)

            for _ in range(40):
                if os.path.exists(SOCKET_PATH):
                    break
                time.sleep(0.25)
            else:
                sys.exit(1)

            send_to_cli(
                "[CONFIG] mode={} profiling={}".format(
                    "release" if RELEASE else "debug",
                    "enabled" if ENABLE_PROFILING else "disabled",
                )
            )

            success, build_output = cargo_build(target, features=target_features)
            block = "--------------------------------------------------------------------- COMPILATION ---------------------------------------------------------------------\n"
            if build_output.strip():
                block += build_output
            block += "-------------------------------------------------------------------------------------------------------------------------------------------------------\n"
            send_to_cli(block)

            if not success:
                cli.wait()  # attend que l'utilisateur ferme le CLI (q / Esc)
                sys.exit(1)

            binary = os.path.join(WORKSPACE, "target", PROFILE, target)
            engine = subprocess.Popen([binary] + engine_args(), cwd=WORKSPACE)
            processes.append(engine)

            engine_running = True
            while cli.poll() is None:
                if engine_running and engine.poll() is not None:
                    engine_running = False
                    send_to_cli(
                        "[INFO] L'engine s'est arrêté (code {})".format(
                            engine.returncode
                        )
                    )
                time.sleep(0.2)
        else:
            engine = cargo_run(
                target, extra_args=engine_args(), features=target_features
            )
            processes.append(engine)
            engine.wait()

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
